use std::cmp::{max, min};
use std::collections::BTreeSet;
use std::convert::TryFrom;
use std::ffi::{c_void, CStr, CString};
use std::fmt::Write;
use std::marker::PhantomData;
use std::mem::size_of;
use std::os::raw::{c_char, c_ulong};
use std::ptr;
use std::slice;

use anyhow::{anyhow, bail, ensure, Result};
use scroll::Pread;

use crate::btf::c_types::*;
use crate::btf::*;
use crate::gen::BpfObj;

const ANON_PREFIX: &str = "__anon_";

fn get_vlen(info: u32) -> u32 {
    info & 0xffff
}

fn get_kind_flag(info: u32) -> bool {
    (info >> 31) == 1
}

fn get_kind(info: u32) -> u32 {
    (info >> 24) & 0x1f
}

struct BtfLoader<'dat> {
    /// Counter for anonymous types.
    anon_count: u32,
    /// Phantom data for the type's lifetime. Necessary only to avoid having to
    /// push it to each method.
    _phantom: PhantomData<&'dat ()>,
}

impl<'dat> BtfLoader<'dat> {
    /// Load all BTF types.
    fn load(type_data: &'dat [u8], string_table: &'dat [u8]) -> Result<Vec<BtfType<'dat>>> {
        let mut slf = Self {
            anon_count: 0,
            _phantom: PhantomData,
        };
        slf.load_now(type_data, string_table)
    }

    fn load_now(
        &mut self,
        type_data: &'dat [u8],
        string_table: &'dat [u8],
    ) -> Result<Vec<BtfType<'dat>>> {
        let mut off: usize = 0;
        // Type ID 0 is reserved for Void
        let mut types = vec![BtfType::Void];

        while off < type_data.len() {
            let t = self.load_type(string_table, &type_data[off..])?;
            off += Self::type_size(&t);
            types.push(t);
        }

        Ok(types)
    }

    fn load_type(&mut self, string_table: &'dat [u8], data: &'dat [u8]) -> Result<BtfType<'dat>> {
        let t = data.pread::<btf_type>(0)?;
        let extra = &data[size_of::<btf_type>()..];
        let kind = get_kind(t.info);

        match BtfKind::try_from(kind)? {
            BtfKind::Void => {
                let _ = BtfType::Void; // Silence unused variant warning
                bail!("Cannot load Void type");
            }
            BtfKind::Int => Self::load_int(string_table, &t, extra),
            BtfKind::Float => Self::load_float(string_table, &t),
            BtfKind::Ptr => Ok(BtfType::Ptr(BtfPtr {
                pointee_type: t.type_id,
            })),
            BtfKind::Array => Self::load_array(extra),
            BtfKind::Struct => self.load_struct(string_table, &t, extra),
            BtfKind::Union => self.load_union(string_table, &t, extra),
            BtfKind::Enum => self.load_enum(string_table, &t, extra),
            BtfKind::Enum64 => self.load_enum64(string_table, &t, extra),
            BtfKind::Fwd => Self::load_fwd(string_table, &t),
            BtfKind::Typedef => Ok(BtfType::Typedef(BtfTypedef {
                name: Self::get_btf_str(string_table, t.name_off as usize)?,
                type_id: t.type_id,
            })),
            BtfKind::Volatile => Ok(BtfType::Volatile(BtfVolatile { type_id: t.type_id })),
            BtfKind::Const => Ok(BtfType::Const(BtfConst { type_id: t.type_id })),
            BtfKind::Restrict => Ok(BtfType::Restrict(BtfRestrict { type_id: t.type_id })),
            BtfKind::Func => Ok(BtfType::Func(BtfFunc {
                name: Self::get_btf_str(string_table, t.name_off as usize)?,
                type_id: t.type_id,
            })),
            BtfKind::FuncProto => Self::load_func_proto(string_table, &t, extra),
            BtfKind::Var => Self::load_var(string_table, &t, extra),
            BtfKind::Datasec => Self::load_datasec(string_table, &t, extra),
            BtfKind::DeclTag => Self::load_decl_tag(string_table, &t, extra),
            BtfKind::TypeTag => Ok(BtfType::TypeTag(BtfTypeTag {
                name: Self::get_btf_str(string_table, t.name_off as usize)?,
                type_id: t.type_id,
            })),
        }
    }

    fn load_int(
        string_table: &'dat [u8],
        t: &btf_type,
        extra: &'dat [u8],
    ) -> Result<BtfType<'dat>> {
        let info = extra.pread::<u32>(0)?;
        let enc: u8 = ((info >> 24) & 0xf) as u8;
        let off: u8 = ((info >> 16) & 0xff) as u8;
        let bits: u8 = (info & 0xff) as u8;
        Ok(BtfType::Int(BtfInt {
            name: Self::get_btf_str(string_table, t.name_off as usize)?,
            bits,
            offset: off,
            encoding: BtfIntEncoding::try_from(enc)?,
        }))
    }

    fn load_float(string_table: &'dat [u8], t: &btf_type) -> Result<BtfType<'dat>> {
        Ok(BtfType::Float(BtfFloat {
            name: Self::get_btf_str(string_table, t.name_off as usize)?,
            size: t.type_id,
        }))
    }

    fn load_array(extra: &'dat [u8]) -> Result<BtfType<'dat>> {
        let info = extra.pread::<btf_array>(0)?;
        Ok(BtfType::Array(BtfArray {
            nelems: info.nelems,
            index_type_id: info.idx_type_id,
            val_type_id: info.val_type_id,
        }))
    }

    fn load_struct(
        &mut self,
        string_table: &'dat [u8],
        t: &btf_type,
        extra: &'dat [u8],
    ) -> Result<BtfType<'dat>> {
        let name = match Self::get_btf_str(string_table, t.name_off as usize)? {
            "" => {
                self.anon_count += 1;
                format!("{}{}", ANON_PREFIX, self.anon_count)
            }
            n => n.to_string(),
        };
        Ok(BtfType::Struct(BtfComposite {
            name,
            is_struct: true,
            size: t.type_id,
            members: Self::load_members(string_table, t, extra)?,
        }))
    }

    fn load_union(
        &mut self,
        string_table: &'dat [u8],
        t: &btf_type,
        extra: &'dat [u8],
    ) -> Result<BtfType<'dat>> {
        let name = match Self::get_btf_str(string_table, t.name_off as usize)? {
            "" => {
                self.anon_count += 1;
                format!("{}{}", ANON_PREFIX, self.anon_count)
            }
            n => n.to_string(),
        };
        Ok(BtfType::Union(BtfComposite {
            name,
            is_struct: false,
            size: t.type_id,
            members: Self::load_members(string_table, t, extra)?,
        }))
    }

    fn load_members(
        string_table: &'dat [u8],
        t: &btf_type,
        extra: &'dat [u8],
    ) -> Result<Vec<BtfMember<'dat>>> {
        let mut res = Vec::new();
        let mut off: usize = 0;
        let bits = get_kind_flag(t.info);

        for _ in 0..get_vlen(t.info) {
            let m = extra.pread::<btf_member>(off)?;
            res.push(BtfMember {
                name: Self::get_btf_str(string_table, m.name_off as usize)?,
                type_id: m.type_id,
                bit_size: if bits { (m.offset >> 24) as u8 } else { 0 },
                bit_offset: if bits { m.offset & 0xffffff } else { m.offset },
            });

            off += size_of::<btf_member>();
        }

        Ok(res)
    }

    fn load_enum(
        &mut self,
        string_table: &'dat [u8],
        t: &btf_type,
        extra: &'dat [u8],
    ) -> Result<BtfType<'dat>> {
        let name = match Self::get_btf_str(string_table, t.name_off as usize)? {
            "" => {
                self.anon_count += 1;
                format!("{}{}", ANON_PREFIX, self.anon_count)
            }
            n => n.to_string(),
        };

        let mut vals = Vec::new();
        let mut off: usize = 0;
        for _ in 0..get_vlen(t.info) {
            let v = extra.pread::<btf_enum>(off)?;
            vals.push(BtfEnumValue {
                name: Self::get_btf_str(string_table, v.name_off as usize)?,
                value: v.val,
            });

            off += size_of::<btf_enum>();
        }

        Ok(BtfType::Enum(BtfEnum {
            name,
            size: t.type_id,
            values: vals,
        }))
    }

    fn load_enum64(
        &mut self,
        string_table: &'dat [u8],
        t: &btf_type,
        extra: &'dat [u8],
    ) -> Result<BtfType<'dat>> {
        let name = match Self::get_btf_str(string_table, t.name_off as usize)? {
            "" => {
                self.anon_count += 1;
                format!("{}{}", ANON_PREFIX, self.anon_count)
            }
            n => n.to_string(),
        };

        let mut vals = Vec::new();
        let mut off: usize = 0;
        for _ in 0..get_vlen(t.info) {
            let v = extra.pread::<btf_enum64>(off)?;
            vals.push(BtfEnum64Value {
                name: Self::get_btf_str(string_table, v.name_off as usize)?,
                value: ((v.val_hi32 as u64) << 32) | (v.val_lo32 as u64),
            });

            off += size_of::<btf_enum64>();
        }

        Ok(BtfType::Enum64(BtfEnum64 {
            name,
            size: t.type_id,
            signed: get_kind_flag(t.info),
            values: vals,
        }))
    }

    fn load_fwd(string_table: &'dat [u8], t: &btf_type) -> Result<BtfType<'dat>> {
        Ok(BtfType::Fwd(BtfFwd {
            name: Self::get_btf_str(string_table, t.name_off as usize)?,
            kind: if get_kind_flag(t.info) {
                BtfFwdKind::Union
            } else {
                BtfFwdKind::Struct
            },
        }))
    }

    fn load_func_proto(
        string_table: &'dat [u8],
        t: &btf_type,
        extra: &'dat [u8],
    ) -> Result<BtfType<'dat>> {
        let mut params = Vec::new();
        let mut off: usize = 0;

        for _ in 0..get_vlen(t.info) {
            let p = extra.pread::<btf_param>(off)?;
            params.push(BtfFuncParam {
                name: Self::get_btf_str(string_table, p.name_off as usize)?,
                type_id: p.type_id,
            });

            off += size_of::<btf_param>();
        }

        Ok(BtfType::FuncProto(BtfFuncProto {
            ret_type_id: t.type_id,
            params,
        }))
    }

    fn load_var(
        string_table: &'dat [u8],
        t: &btf_type,
        extra: &'dat [u8],
    ) -> Result<BtfType<'dat>> {
        let kind = extra.pread::<u32>(0)?;
        Ok(BtfType::Var(BtfVar {
            name: Self::get_btf_str(string_table, t.name_off as usize)?,
            type_id: t.type_id,
            linkage: BtfVarLinkage::try_from(kind)?,
        }))
    }

    fn load_datasec(
        string_table: &'dat [u8],
        t: &btf_type,
        extra: &'dat [u8],
    ) -> Result<BtfType<'dat>> {
        let mut vars = Vec::new();
        let mut off: usize = 0;

        for _ in 0..get_vlen(t.info) {
            let v = extra.pread::<btf_datasec_var>(off)?;
            vars.push(BtfDatasecVar {
                type_id: v.type_id,
                offset: v.offset,
                size: v.size,
            });

            off += size_of::<btf_datasec_var>();
        }

        Ok(BtfType::Datasec(BtfDatasec {
            name: Self::get_btf_str(string_table, t.name_off as usize)?,
            size: t.type_id,
            vars,
        }))
    }

    fn load_decl_tag(
        string_table: &'dat [u8],
        t: &btf_type,
        extra: &'dat [u8],
    ) -> Result<BtfType<'dat>> {
        let decl_tag = extra.pread::<btf_decl_tag>(0)?;
        Ok(BtfType::DeclTag(BtfDeclTag {
            name: Self::get_btf_str(string_table, t.name_off as usize)?,
            type_id: t.type_id,
            component_idx: decl_tag.component_idx,
        }))
    }

    /// Returns size of type on disk in .BTF section
    fn type_size(t: &BtfType) -> usize {
        let common = size_of::<btf_type>();
        match t {
            BtfType::Void => 0,
            BtfType::Ptr(_)
            | BtfType::Fwd(_)
            | BtfType::Typedef(_)
            | BtfType::Volatile(_)
            | BtfType::Const(_)
            | BtfType::Restrict(_)
            | BtfType::Func(_)
            | BtfType::Float(_)
            | BtfType::TypeTag(_) => common,
            BtfType::Int(_) | BtfType::Var(_) => common + size_of::<u32>(),
            BtfType::Array(_) => common + size_of::<btf_array>(),
            BtfType::Struct(t) => common + t.members.len() * size_of::<btf_member>(),
            BtfType::Union(t) => common + t.members.len() * size_of::<btf_member>(),
            BtfType::Enum(t) => common + t.values.len() * size_of::<btf_enum>(),
            BtfType::Enum64(t) => common + t.values.len() * size_of::<btf_enum64>(),
            BtfType::FuncProto(t) => common + t.params.len() * size_of::<btf_param>(),
            BtfType::Datasec(t) => common + t.vars.len() * size_of::<btf_datasec_var>(),
            BtfType::DeclTag(_) => common + size_of::<btf_decl_tag>(),
        }
    }

    fn get_btf_str(string_table: &[u8], offset: usize) -> Result<&str> {
        let c_str = unsafe { CStr::from_ptr(&string_table[offset] as *const u8 as *const c_char) };
        Ok(c_str.to_str()?)
    }
}

pub struct Btf {
    /// SAFETY: We must not hand out references with a 'static lifetime to
    ///         this member. They should never outlive `self`.
    types: Vec<BtfType<'static>>,
    ptr_size: u32,
    /// Copy of the raw BTF data from the BPF object.
    ///
    /// SAFETY: Needs to stay last to be dropped last, as other members
    ///         reference it. We also must not move out of it while references
    ///         to it are present.
    _raw_data: Box<[u8]>,
}

impl Btf {
    pub fn new(name: &str, object_file: &[u8]) -> Result<Option<Self>> {
        let cname = CString::new(name)?;
        let obj_opts = libbpf_sys::bpf_object_open_opts {
            sz: std::mem::size_of::<libbpf_sys::bpf_object_open_opts>() as libbpf_sys::size_t,
            object_name: cname.as_ptr(),
            ..Default::default()
        };
        let bpf_obj = unsafe {
            libbpf_sys::bpf_object__open_mem(
                object_file.as_ptr() as *const c_void,
                object_file.len() as c_ulong,
                &obj_opts,
            )
        };

        ensure!(!bpf_obj.is_null(), "Failed to bpf_object__open_mem");

        let err = unsafe { libbpf_sys::libbpf_get_error(bpf_obj as *const _) };
        ensure!(err == 0, "Failed to bpf_object__open_mem: errno {}", err);

        let mut bpf_obj = BpfObj::new(ptr::NonNull::new(bpf_obj).unwrap());
        let bpf_obj_btf = unsafe { libbpf_sys::bpf_object__btf(bpf_obj.as_mut_ptr()) };
        if bpf_obj_btf.is_null() {
            return Ok(None);
        }

        let num: u32 = 0x1234;
        let endianness = if num.to_le_bytes() == num.to_ne_bytes() {
            libbpf_sys::BTF_LITTLE_ENDIAN
        } else {
            libbpf_sys::BTF_BIG_ENDIAN
        };
        ensure!(
            unsafe { libbpf_sys::btf__set_endianness(bpf_obj_btf, endianness) } == 0,
            "Failed to set BTF endianness"
        );

        let ptr_size = unsafe { libbpf_sys::btf__pointer_size(bpf_obj_btf) };
        ensure!(ptr_size != 0, "Could not determine BTF pointer size");

        let mut raw_data_size = 0;
        let raw_data = unsafe { libbpf_sys::btf__raw_data(bpf_obj_btf, &mut raw_data_size) };
        ensure!(
            !raw_data.is_null() && raw_data_size > 0,
            "Could not get raw BTF data"
        );
        let raw_data_copy =
            unsafe { slice::from_raw_parts(raw_data as *const u8, raw_data_size as usize) }
                .to_vec()
                .into_boxed_slice();

        // `data` is valid as long as `raw_data_copy` is valid, so we're safe to
        // conjure up this `'static` lifetime, as long as we make sure that
        // references carrying it do not leave the `Btf` object itself.
        let data: &'static [u8] = unsafe {
            slice::from_raw_parts(raw_data_copy.as_ptr() as *const u8, raw_data_size as usize)
        };

        // Read section header
        let hdr = data.pread::<btf_header>(0)?;
        ensure!(hdr.magic == BTF_MAGIC, "Invalid BTF magic");
        ensure!(
            hdr.version == BTF_VERSION,
            "Unsupported BTF version: {}",
            hdr.version
        );

        // String table
        let str_off = (hdr.hdr_len + hdr.str_off) as usize;
        let str_end = str_off + (hdr.str_len as usize);
        ensure!(str_end <= data.len(), "String table out of bounds");
        let str_data = &data[str_off..str_end];

        // Type table
        let type_off = (hdr.hdr_len + hdr.type_off) as usize;
        let type_end = type_off + (hdr.type_len as usize);
        ensure!(type_end <= data.len(), "Type table out of bounds");
        let type_data = &data[type_off..type_end];

        let btf = Btf {
            _raw_data: raw_data_copy,
            types: BtfLoader::load(type_data, str_data)?,
            ptr_size: ptr_size as u32,
        };

        Ok(Some(btf))
    }

    pub fn types(&self) -> &[BtfType<'_>] {
        &self.types
    }

    pub fn type_by_id(&self, type_id: u32) -> Result<&BtfType> {
        if (type_id as usize) < self.types.len() {
            Ok(&self.types[type_id as usize])
        } else {
            bail!("Invalid type_id: {}", type_id);
        }
    }

    pub fn size_of(&self, type_id: u32) -> Result<u32> {
        let skipped_type_id = self.skip_mods_and_typedefs(type_id)?;

        Ok(match self.type_by_id(skipped_type_id)? {
            BtfType::Int(t) => ((t.bits + 7) / 8).into(),
            BtfType::Ptr(_) => self.ptr_size,
            BtfType::Array(t) => t.nelems * self.size_of(t.val_type_id)?,
            BtfType::Struct(t) => t.size,
            BtfType::Union(t) => t.size,
            BtfType::Enum(t) => t.size,
            BtfType::Enum64(t) => t.size,
            BtfType::Var(t) => self.size_of(t.type_id)?,
            BtfType::Datasec(t) => t.size,
            BtfType::Float(t) => t.size,
            BtfType::Void
            | BtfType::Volatile(_)
            | BtfType::Const(_)
            | BtfType::Restrict(_)
            | BtfType::Typedef(_)
            | BtfType::FuncProto(_)
            | BtfType::Fwd(_)
            | BtfType::Func(_)
            | BtfType::DeclTag(_)
            | BtfType::TypeTag(_) => bail!("Cannot get size of type_id: {}", skipped_type_id),
        })
    }

    pub fn align_of(&self, type_id: u32) -> Result<u32> {
        let skipped_type_id = self.skip_mods_and_typedefs(type_id)?;

        Ok(match self.type_by_id(skipped_type_id)? {
            BtfType::Int(t) => min(self.ptr_size, ((t.bits + 7) / 8).into()),
            BtfType::Ptr(_) => self.ptr_size,
            BtfType::Array(t) => self.align_of(t.val_type_id)?,
            BtfType::Struct(t) | BtfType::Union(t) => {
                let mut align = 1;
                for m in &t.members {
                    align = max(align, self.align_of(m.type_id)?);
                }

                align
            }
            BtfType::Enum(t) => min(self.ptr_size, t.size),
            BtfType::Enum64(t) => min(self.ptr_size, t.size),
            BtfType::Var(t) => self.align_of(t.type_id)?,
            BtfType::Datasec(t) => t.size,
            BtfType::Float(t) => min(self.ptr_size, t.size),
            BtfType::Void
            | BtfType::Volatile(_)
            | BtfType::Const(_)
            | BtfType::Restrict(_)
            | BtfType::Typedef(_)
            | BtfType::FuncProto(_)
            | BtfType::Fwd(_)
            | BtfType::Func(_)
            | BtfType::DeclTag(_)
            | BtfType::TypeTag(_) => bail!("Cannot get alignment of type_id: {}", skipped_type_id),
        })
    }

    /// Returns the rust-ified type declaration of `ty` in string format.
    ///
    /// Rule of thumb is `ty` must be a type a variable can have.
    ///
    /// Type qualifiers are discarded (eg `const`, `volatile`, etc).
    pub fn type_declaration(&self, type_id: u32) -> Result<String> {
        let stripped_type_id = self.skip_mods_and_typedefs(type_id)?;
        let ty = self.type_by_id(stripped_type_id)?;

        Ok(match ty {
            BtfType::Void => "std::ffi::c_void".to_string(),
            BtfType::Int(t) => {
                let width = match (t.bits + 7) / 8 {
                    1 => "8",
                    2 => "16",
                    4 => "32",
                    8 => "64",
                    16 => "128",
                    _ => bail!("Invalid integer width"),
                };

                match t.encoding {
                    btf::BtfIntEncoding::Signed => format!("i{width}"),
                    btf::BtfIntEncoding::Bool => {
                        assert!(t.bits as usize == (std::mem::size_of::<bool>() * 8));
                        "bool".to_string()
                    }
                    btf::BtfIntEncoding::Char | btf::BtfIntEncoding::None => format!("u{width}"),
                }
            }
            BtfType::Float(t) => {
                let width = match t.size {
                    2 => bail!("Unsupported float width"),
                    4 => "32",
                    8 => "64",
                    12 => bail!("Unsupported float width"),
                    16 => bail!("Unsupported float width"),
                    _ => bail!("Invalid float width"),
                };

                format!("f{width}")
            }
            BtfType::Ptr(t) => {
                let pointee_ty = self.type_declaration(t.pointee_type)?;

                format!("*mut {pointee_ty}")
            }
            BtfType::Array(t) => {
                let val_ty = self.type_declaration(t.val_type_id)?;

                format!("[{}; {}]", val_ty, t.nelems)
            }
            BtfType::Struct(t) | BtfType::Union(t) => t.name.to_string(),
            BtfType::Enum(t) => t.name.to_string(),
            BtfType::Enum64(t) => t.name.to_string(),
            // The only way a variable references a function is through a function pointer.
            // Return c_void here so the final def will look like `*mut c_void`.
            //
            // It's not like rust code can call a function inside a bpf prog either so we don't
            // really need a full definition. `void *` is totally sufficient for sharing a pointer.
            BtfType::Func(_) => "std::ffi::c_void".to_string(),
            BtfType::Var(t) => self.type_declaration(t.type_id)?,
            BtfType::Fwd(_)
            | BtfType::FuncProto(_)
            | BtfType::Datasec(_)
            | BtfType::Typedef(_)
            | BtfType::Volatile(_)
            | BtfType::Const(_)
            | BtfType::Restrict(_)
            | BtfType::DeclTag(_)
            | BtfType::TypeTag(_) => {
                bail!("Invalid type: {}", ty)
            }
        })
    }

    /// Returns an expression that evaluates to the Default value
    /// of a type(typeid) in string form.
    ///
    /// To be used when creating a impl Default for a structure
    ///
    /// Rule of thumb is `ty` must be a type a variable can have.
    ///
    /// Type qualifiers are discarded (eg `const`, `volatile`, etc).
    pub fn type_default(&self, type_id: u32) -> Result<String> {
        let stripped_type_id = self.skip_mods_and_typedefs(type_id)?;
        let ty = self.type_by_id(stripped_type_id)?;

        Ok(match ty {
            BtfType::Void => "std::ffi::c_void::default()".to_string(),
            BtfType::Int(_) => format!("{}::default()", self.type_declaration(stripped_type_id)?),
            BtfType::Float(_) => format!("{}::default()", self.type_declaration(stripped_type_id)?),
            BtfType::Ptr(_) => "std::ptr::null_mut()".to_string(),
            BtfType::Array(t) => {
                format!(
                    "[{}; {}]",
                    self.type_default(t.val_type_id)
                        .map_err(|err| anyhow!("in {ty}: {err}"))?,
                    t.nelems
                )
            }
            BtfType::Struct(t) | BtfType::Union(t) => format!("{}::default()", t.name),
            BtfType::Enum(t) => format!("{}::default()", t.name),
            BtfType::Enum64(t) => format!("{}::default()", t.name),
            BtfType::Var(t) => format!("{}::default()", self.type_declaration(t.type_id)?),
            BtfType::Func(_)
            | BtfType::Fwd(_)
            | BtfType::FuncProto(_)
            | BtfType::Datasec(_)
            | BtfType::Typedef(_)
            | BtfType::Volatile(_)
            | BtfType::Const(_)
            | BtfType::Restrict(_)
            | BtfType::DeclTag(_)
            | BtfType::TypeTag(_) => {
                bail!("Invalid type: {}", ty)
            }
        })
    }

    fn is_struct_packed(&self, struct_type_id: u32, t: &BtfComposite) -> Result<bool> {
        if !t.is_struct {
            return Ok(false);
        }

        let align = self.align_of(struct_type_id)?;
        ensure!(
            align != 0,
            "Failed to get alignment of struct_type_id: {}",
            struct_type_id
        );

        // Size of a struct has to be a multiple of its alignment
        if t.size % align != 0 {
            return Ok(true);
        }

        // All the non-bitfield fields have to be naturally aligned
        for m in &t.members {
            let align = self.align_of(m.type_id)?;
            ensure!(
                align != 0,
                "Failed to get alignment of m.type_id: {}",
                m.type_id
            );

            if m.bit_size == 0 && m.bit_offset % (align * 8) != 0 {
                return Ok(true);
            }
        }

        // Even if original struct was marked as packed, we haven't detected any misalignment, so
        // there is no effect of packedness for given struct
        Ok(false)
    }

    /// Given a `current_offset` (in bytes) into a struct and a `required_offset` (in bytes) that
    /// type `type_id` needs to be placed at, returns how much padding must be inserted before
    /// `type_id`.
    fn required_padding(
        &self,
        current_offset: usize,
        required_offset: usize,
        type_id: u32,
        packed: bool,
    ) -> Result<usize> {
        ensure!(
            current_offset <= required_offset,
            "Current offset ahead of required offset"
        );

        let align = if packed {
            1
        } else {
            // Assume 32-bit alignment in case we're generating code for 32-bit
            // arch. Worst case is on a 64-bit arch the compiler will generate
            // extra padding. The final layout will still be identical to what is
            // described by BTF.
            let a = self.align_of(type_id)? as usize;
            ensure!(a != 0, "Failed to get alignment of type_id: {}", type_id);

            if a > 4 {
                4
            } else {
                a
            }
        };

        // If we aren't aligning to the natural offset, padding needs to be inserted
        let aligned_offset = (current_offset + align - 1) / align * align;
        if aligned_offset == required_offset {
            Ok(0)
        } else {
            Ok(required_offset - current_offset)
        }
    }

    /// Returns rust type definition of `ty` in string format, including dependent types.
    ///
    /// `ty` must be a struct, union, enum, or datasec type.
    pub fn type_definition(&self, type_id: u32) -> Result<String> {
        let next_type = |mut id| -> Result<Option<u32>> {
            loop {
                match self.type_by_id(id)? {
                    BtfType::Struct(_)
                    | BtfType::Union(_)
                    | BtfType::Enum(_)
                    | BtfType::Enum64(_)
                    | BtfType::Datasec(_) => return Ok(Some(id)),
                    BtfType::Ptr(t) => id = t.pointee_type,
                    BtfType::Array(t) => id = t.val_type_id,
                    BtfType::Volatile(t) => id = t.type_id,
                    BtfType::Const(t) => id = t.type_id,
                    BtfType::Restrict(t) => id = t.type_id,
                    BtfType::Typedef(t) => id = t.type_id,
                    BtfType::DeclTag(t) => id = t.type_id,
                    BtfType::TypeTag(t) => id = t.type_id,
                    BtfType::Void
                    | BtfType::Int(_)
                    | BtfType::Float(_)
                    | BtfType::Fwd(_)
                    | BtfType::Func(_)
                    | BtfType::FuncProto(_)
                    | BtfType::Var(_) => return Ok(None),
                }
            }
        };

        let is_terminal = |id| -> Result<bool> {
            match self.type_by_id(id)?.kind() {
                BtfKind::Struct
                | BtfKind::Union
                | BtfKind::Enum
                | BtfKind::Enum64
                | BtfKind::Datasec => Ok(false),
                BtfKind::Void
                | BtfKind::Int
                | BtfKind::Float
                | BtfKind::Ptr
                | BtfKind::Array
                | BtfKind::Fwd
                | BtfKind::Typedef
                | BtfKind::Volatile
                | BtfKind::Const
                | BtfKind::Restrict
                | BtfKind::Func
                | BtfKind::FuncProto
                | BtfKind::Var
                | BtfKind::DeclTag
                | BtfKind::TypeTag => Ok(true),
            }
        };

        ensure!(
            !is_terminal(type_id)?,
            "Tried to print type definition for terminal type"
        );

        // Process dependent types until there are none left.
        //
        // When we hit a terminal, we write out some stuff. A non-terminal adds more types to
        // the queue.
        let mut def = String::new();
        let mut dependent_types = vec![type_id];
        let mut processed = BTreeSet::new();
        while !dependent_types.is_empty() {
            let type_id = dependent_types.remove(0);
            if processed.contains(&type_id) {
                continue;
            } else {
                processed.insert(type_id);
            }

            let ty = self.type_by_id(type_id)?;

            match ty {
                BtfType::Struct(t) | BtfType::Union(t) => {
                    let packed = self.is_struct_packed(type_id, t)?;

                    // fields in the aggregate
                    let mut agg_content: Vec<String> = Vec::new();

                    // structs with arrays > 32 length need to impl Default
                    // rather than #[derive(Default)]
                    let mut impl_default: Vec<String> = Vec::new(); // output for impl Default
                    let mut gen_impl_default = false; // whether to output impl Default or use #[derive]

                    let mut offset = 0; // In bytes
                    for member in &t.members {
                        ensure!(
                            member.bit_size == 0 && member.bit_offset % 8 == 0,
                            "Struct bitfields not supported"
                        );

                        let field_ty_id = self.skip_mods_and_typedefs(member.type_id)?;
                        if let Some(next_ty_id) = next_type(field_ty_id)? {
                            dependent_types.push(next_ty_id);
                        }

                        // Add padding as necessary
                        if t.is_struct {
                            let padding = self.required_padding(
                                offset,
                                member.bit_offset as usize / 8,
                                member.type_id,
                                packed,
                            )?;

                            if padding != 0 {
                                agg_content
                                    .push(format!(r#"    __pad_{offset}: [u8; {padding}],"#,));

                                impl_default.push(format!(
                                    r#"            __pad_{offset}: [u8::default(); {padding}]"#,
                                ));
                            }

                            if let BtfType::Array(ft) = self.type_by_id(field_ty_id)? {
                                if ft.nelems > 32 {
                                    gen_impl_default = true
                                }
                            }
                        }

                        match self.type_default(field_ty_id) {
                            Ok(def) => {
                                impl_default.push(format!(
                                    r#"            {field_name}: {field_ty_str}"#,
                                    field_name = member.name,
                                    field_ty_str = def
                                ));
                            }
                            Err(e) => {
                                if gen_impl_default || !t.is_struct {
                                    bail!("Could not construct a necessary Default Impl: {}", e);
                                }
                            }
                        };

                        // Set `offset` to end of current var
                        offset = ((member.bit_offset / 8) + self.size_of(field_ty_id)?) as usize;

                        let field_ty_str = self.type_declaration(field_ty_id)?;
                        let field_name = if !member.name.is_empty() {
                            member.name.to_string()
                        } else {
                            field_ty_str.clone()
                        };

                        agg_content.push(format!(r#"    pub {field_name}: {field_ty_str},"#));
                    }

                    if t.is_struct {
                        let struct_size = t.size as usize;
                        let padding =
                            self.required_padding(offset, struct_size, type_id, packed)?;
                        if padding != 0 {
                            agg_content.push(format!(r#"    __pad_{offset}: [u8; {padding}],"#,));
                            impl_default.push(format!(
                                r#"            __pad_{offset}: [u8::default(); {padding}]"#,
                            ));
                        }
                    }

                    if !gen_impl_default && t.is_struct {
                        writeln!(def, r#"#[derive(Debug, Default, Copy, Clone)]"#)?;
                    } else if t.is_struct {
                        writeln!(def, r#"#[derive(Debug, Copy, Clone)]"#)?;
                    } else {
                        writeln!(def, r#"#[derive(Copy, Clone)]"#)?;
                    }

                    let aggregate_type = if t.is_struct { "struct" } else { "union" };
                    let packed_repr = if packed { ", packed" } else { "" };

                    writeln!(def, r#"#[repr(C{packed_repr})]"#)?;
                    writeln!(
                        def,
                        r#"pub {agg_type} {name} {{"#,
                        agg_type = aggregate_type,
                        name = t.name,
                    )?;

                    for field in agg_content {
                        writeln!(def, "{field}")?;
                    }
                    writeln!(def, "}}")?;

                    // if required write a Default implementation for this struct
                    if gen_impl_default {
                        writeln!(def, r#"impl Default for {} {{"#, t.name)?;
                        writeln!(def, r#"    fn default() -> Self {{"#)?;
                        writeln!(def, r#"        {} {{"#, t.name)?;
                        for impl_def in impl_default {
                            writeln!(def, r#"{impl_def},"#)?;
                        }
                        writeln!(def, r#"        }}"#)?;
                        writeln!(def, r#"    }}"#)?;
                        writeln!(def, r#"}}"#)?;
                    } else if !t.is_struct {
                        // write a Debug implementation for a union
                        writeln!(def, r#"impl std::fmt::Debug for {} {{"#, t.name)?;
                        writeln!(
                            def,
                            r#"    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{"#
                        )?;
                        writeln!(def, r#"        write!(f, "(???)")"#)?;
                        writeln!(def, r#"    }}"#)?;
                        writeln!(def, r#"}}"#)?;

                        // write a Default implementation for a union
                        writeln!(def, r#"impl Default for {} {{"#, t.name)?;
                        writeln!(def, r#"    fn default() -> Self {{"#)?;
                        writeln!(def, r#"        {} {{"#, t.name)?;
                        writeln!(def, r#"{},"#, impl_default[0])?;
                        writeln!(def, r#"        }}"#)?;
                        writeln!(def, r#"    }}"#)?;
                        writeln!(def, r#"}}"#)?;
                    }
                }
                BtfType::Enum(t) => {
                    let repr_size = match t.size {
                        1 => "8",
                        2 => "16",
                        4 => "32",
                        8 => "64",
                        16 => "128",
                        _ => bail!("Invalid enum size: {}", t.size),
                    };

                    let mut signed = "u";
                    for value in &t.values {
                        if value.value < 0 {
                            signed = "i";
                            break;
                        }
                    }

                    writeln!(def, r#"#[derive(Debug, Copy, Clone, PartialEq, Eq)]"#)?;
                    writeln!(def, r#"#[repr({signed}{repr_size})]"#)?;
                    writeln!(def, r#"pub enum {name} {{"#, name = t.name)?;

                    for value in &t.values {
                        writeln!(
                            def,
                            r#"    {name} = {value},"#,
                            name = value.name,
                            value = value.value,
                        )?;
                    }

                    writeln!(def, "}}")?;

                    // write an impl Default for this enum
                    if !t.values.is_empty() {
                        // TODO: remove #[allow(clippy::derivable_impls)]
                        //       once minimum rust at 1.62+
                        writeln!(def, r#"#[allow(clippy::derivable_impls)]"#)?;
                        writeln!(def, r#"impl Default for {name} {{"#, name = t.name)?;
                        writeln!(def, r#"    fn default() -> Self {{"#)?;
                        writeln!(
                            def,
                            r#"        {name}::{value}"#,
                            name = t.name,
                            value = t.values[0].name
                        )?;
                        writeln!(def, r#"    }}"#)?;
                        writeln!(def, r#"}}"#)?;
                    }
                }
                BtfType::Enum64(t) => {
                    let repr_size = match t.size {
                        1 => "8",
                        2 => "16",
                        4 => "32",
                        8 => "64",
                        _ => bail!("Invalid enum64 size: {}", t.size),
                    };

                    let signed = if t.signed { "i" } else { "u" };

                    writeln!(def, r#"#[derive(Debug, Copy, Clone, PartialEq, Eq)]"#)?;
                    writeln!(def, r#"#[repr({signed}{repr_size})]"#)?;
                    writeln!(def, r#"pub enum {name} {{"#, name = t.name)?;

                    for value in &t.values {
                        writeln!(
                            def,
                            r#"    {name} = {value},"#,
                            name = value.name,
                            value = value.value,
                        )?;
                    }

                    writeln!(def, "}}")?;

                    // write an impl Default for this enum
                    if !t.values.is_empty() {
                        // TODO: remove #[allow(clippy::derivable_impls)]
                        //       once minimum rust at 1.62+
                        writeln!(def, r#"#[allow(clippy::derivable_impls)]"#)?;
                        writeln!(def, r#"impl Default for {name} {{"#, name = t.name)?;
                        writeln!(def, r#"    fn default() -> Self {{"#)?;
                        writeln!(
                            def,
                            r#"        {name}::{value}"#,
                            name = t.name,
                            value = t.values[0].name
                        )?;
                        writeln!(def, r#"    }}"#)?;
                        writeln!(def, r#"}}"#)?;
                    }
                }
                BtfType::Datasec(t) => {
                    let mut sec_name = t.name.to_string();
                    if sec_name.is_empty() || !sec_name.starts_with('.') {
                        bail!("Datasec name is invalid: {}", sec_name);
                    }
                    sec_name.remove(0);

                    writeln!(def, r#"#[derive(Debug, Copy, Clone)]"#)?;
                    writeln!(def, r#"#[repr(C)]"#)?;
                    writeln!(def, r#"pub struct {sec_name} {{"#)?;

                    let mut offset: u32 = 0;
                    for datasec_var in &t.vars {
                        let var = match self.type_by_id(datasec_var.type_id)? {
                            BtfType::Var(v) => {
                                if v.linkage == BtfVarLinkage::Static {
                                    // do not output Static Var
                                    continue;
                                }

                                if let Some(next_ty_id) = next_type(v.type_id)? {
                                    dependent_types.push(next_ty_id);
                                }

                                v
                            }
                            _ => bail!("BTF is invalid! Datasec var does not point to a var"),
                        };

                        let padding = self.required_padding(
                            offset as usize,
                            datasec_var.offset as usize,
                            var.type_id,
                            false,
                        )?;
                        if padding != 0 {
                            writeln!(def, r#"    __pad_{offset}: [u8; {padding}],"#)?;
                        }

                        // Set `offset` to end of current var
                        offset = datasec_var.offset + datasec_var.size;

                        writeln!(
                            def,
                            r#"    pub {var_name}: {var_type},"#,
                            var_name = var.name,
                            var_type = self.type_declaration(var.type_id)?
                        )?;
                    }

                    writeln!(def, "}}")?;
                }
                BtfType::Void
                | BtfType::Ptr(_)
                | BtfType::Func(_)
                | BtfType::Int(_)
                | BtfType::Float(_)
                | BtfType::Array(_)
                | BtfType::Fwd(_)
                | BtfType::Typedef(_)
                | BtfType::FuncProto(_)
                | BtfType::Var(_)
                | BtfType::Volatile(_)
                | BtfType::Const(_)
                | BtfType::Restrict(_)
                | BtfType::DeclTag(_)
                | BtfType::TypeTag(_) => bail!("Invalid type: {}", ty),
            }
        }

        Ok(def)
    }

    pub fn skip_mods_and_typedefs(&self, mut type_id: u32) -> Result<u32> {
        loop {
            match self.type_by_id(type_id)? {
                BtfType::Volatile(t) => type_id = t.type_id,
                BtfType::Const(t) => type_id = t.type_id,
                BtfType::Restrict(t) => type_id = t.type_id,
                BtfType::Typedef(t) => type_id = t.type_id,
                BtfType::TypeTag(t) => type_id = t.type_id,
                BtfType::Void
                | BtfType::Int(_)
                | BtfType::Float(_)
                | BtfType::Ptr(_)
                | BtfType::Array(_)
                | BtfType::Struct(_)
                | BtfType::Union(_)
                | BtfType::Enum(_)
                | BtfType::Enum64(_)
                | BtfType::Fwd(_)
                | BtfType::Func(_)
                | BtfType::FuncProto(_)
                | BtfType::Var(_)
                | BtfType::Datasec(_)
                | BtfType::DeclTag(_) => return Ok(type_id),
            };
        }
    }
}
