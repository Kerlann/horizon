//! Rust types of the mod API's signatures, resolved to the shapes that cross the WebAssembly
//! boundary.
use crate::index::{Found, Index, ItemId};
use quote::ToTokens;
use std::collections::HashMap;
use syn::{Fields, GenericArgument, Item, PathArguments, Type};

/// A Rust type as it crosses the shared-memory boundary.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Ty {
    Unit,
    /// A primitive, by its Rust spelling.
    Prim(&'static str),
    /// `u128`/`i128`, as two halves.
    Wide(&'static str),
    /// `std::time::Duration`, as nanoseconds.
    Duration,
    /// `String`, `&str`, `Box<str>`, `Arc<str>`: owned once lifted, except `&'static str`.
    Str(Str),
    List(Box<Ty>, List),
    Option(Box<Ty>),
    Result(Box<Ty>, Box<Ty>),
    Tuple(Vec<Ty>),
    Array(Box<Ty>, usize),
    /// `&T` or `&mut T`.
    Ref(Box<Ty>, bool),
    Boxed(Box<Ty>),
    /// `Arc<T>`, crossing as `T`.
    Shared(Box<Ty>),
    Named(usize),
    /// `&dyn Trait` or `&mut dyn Trait` (the reference is part of it): a frame resource.
    Dyn(String, bool),
    /// `Arc<dyn Trait>` or `Box<dyn Trait>` (`boxed`) of an exported trait: a handler the mod
    /// keeps, crossing as its index.
    Handler(String, bool),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Str {
    Owned,
    Borrowed,
    Static,
    Boxed,
    Shared,
    /// `Cow<'static, str>`.
    Cow,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum List {
    Vec,
    /// A map, as its container names it, crossing as a list of pairs.
    Map(&'static str),
    /// A set, as its container names it.
    Set(&'static str),
    Slice,
    Boxed,
    Shared,
}

/// A named type's definition.
#[derive(Debug)]
pub(crate) struct Def {
    /// Its path as the generated glue names it (`mod_api::NativeBlockClick`).
    pub(super) rust: String,
    /// Its lifetime parameters: it borrows when there are any.
    pub(super) lifetimes: usize,
    /// It implements `Clone`, so a point record can hold a copy of a borrowed one.
    pub(super) clone: bool,
    pub(super) borrowed: BorrowedShape,
    pub(super) append: Option<super::append::Append>,
    pub(super) projection: Option<Vec<Field>>,
    pub(super) view: Option<View>,
}

impl Def {
    pub(super) fn borrow_fields(&self) -> Option<std::borrow::Cow<'_, [Field]>> {
        if let Some(fields) = &self.projection {
            return Some(std::borrow::Cow::Borrowed(fields));
        }
        if let Some(append) = &self.append {
            Some(std::borrow::Cow::Borrowed(&append.fields))
        } else {
            self.borrowed.fields()
        }
    }
}
#[derive(Debug)]
pub(super) struct View {
    pub(super) field: String,
    pub(super) pointee: Ty,
    pub(super) mutable: bool,
    pub(super) snapshot: Ty,
}

#[derive(Debug)]
pub(crate) enum BorrowedShape {
    Record(Vec<Field>),
    /// A tuple struct with public fields.
    TupleRecord(Vec<Ty>),
    Variant(Vec<Case>),
    /// An owned value: its type owns the Wire implementation.
    None,
}

impl BorrowedShape {
    pub(super) fn fields(&self) -> Option<std::borrow::Cow<'_, [Field]>> {
        match self {
            Self::Record(fields) => Some(std::borrow::Cow::Borrowed(fields)),
            Self::TupleRecord(fields) => Some(std::borrow::Cow::Owned(
                fields
                    .iter()
                    .enumerate()
                    .map(|(i, ty)| Field {
                        rust: i.to_string(),
                        ty: ty.clone(),
                    })
                    .collect(),
            )),
            _ => None,
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Field {
    pub(super) rust: String,
    pub(super) ty: Ty,
}

#[derive(Clone, Debug)]
pub(crate) struct Case {
    pub(super) rust: String,
    pub(super) payload: Payload,
}

#[derive(Clone, Debug)]
pub(crate) enum Payload {
    None,
    Tuple(Vec<Ty>),
    /// Named variant fields.
    Record(Vec<Field>),
}

/// The resolved types, by definition.
pub(crate) struct Types<'a> {
    pub(super) index: Index<'a>,
    pub(super) defs: Vec<Def>,
    pub(super) by_item: HashMap<ItemId, usize>,
    /// The `mod_api` root module, where re-exported names are looked up.
    api_root: usize,
    /// Each exported trait with the exported traits its handlers answer as (`super::views`).
    pub(super) views: Vec<(String, Vec<String>)>,
    /// The definition being read, which `Self` in its fields names.
    current: Option<usize>,
}

impl<'a> Types<'a> {
    pub(super) fn new(mut index: Index<'a>) -> Result<Self, String> {
        let api_root = index.module("mod_api", &[])?;
        Ok(Self {
            index,
            views: Vec::new(),
            current: None,
            defs: Vec::new(),
            by_item: HashMap::new(),
            api_root,
        })
    }

    /// `ty`, written in `module`, with `subst` giving the generic parameters in scope.
    pub(super) fn ty(
        &mut self,
        module: usize,
        ty: &Type,
        subst: &[(String, Ty)],
    ) -> Result<Ty, String> {
        Ok(match ty {
            Type::Paren(ty) => self.ty(module, &ty.elem, subst)?,
            Type::Group(ty) => self.ty(module, &ty.elem, subst)?,
            Type::Tuple(tuple) if tuple.elems.is_empty() => Ty::Unit,
            Type::Tuple(tuple) => Ty::Tuple(
                tuple
                    .elems
                    .iter()
                    .map(|ty| self.ty(module, ty, subst))
                    .collect::<Result<_, _>>()?,
            ),
            Type::Array(array) => {
                let len = match &array.len {
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Int(len),
                        ..
                    }) => len.base10_parse().map_err(|e| e.to_string())?,
                    len => return Err(format!("array length {}", len.to_token_stream())),
                };
                Ty::Array(Box::new(self.ty(module, &array.elem, subst)?), len)
            }
            Type::Slice(slice) => {
                Ty::List(Box::new(self.ty(module, &slice.elem, subst)?), List::Slice)
            }
            Type::Reference(reference) => {
                let mutable = reference.mutability.is_some();
                let is_static = reference
                    .lifetime
                    .as_ref()
                    .is_some_and(|lifetime| lifetime.ident == "static");
                match &*reference.elem {
                    Type::TraitObject(object) => Ty::Dyn(trait_name(object)?, mutable),
                    Type::Path(path) if path.path.is_ident("str") => Ty::Str(if is_static {
                        Str::Static
                    } else {
                        Str::Borrowed
                    }),
                    Type::Slice(slice) if !mutable => {
                        Ty::List(Box::new(self.ty(module, &slice.elem, subst)?), List::Slice)
                    }
                    elem => Ty::Ref(Box::new(self.ty(module, elem, subst)?), mutable),
                }
            }
            Type::Path(path) if path.qself.is_none() => self.path(module, &path.path, subst)?,
            // A trait object behind a box or `Arc` (`Box<T>` with `T = dyn Handler`).
            Type::TraitObject(object) => Ty::Dyn(trait_name(object)?, false),
            ty => return Err(format!("unsupported type {}", ty.to_token_stream())),
        })
    }

    fn path(
        &mut self,
        module: usize,
        path: &syn::Path,
        subst: &[(String, Ty)],
    ) -> Result<Ty, String> {
        let last = path.segments.last().ok_or("empty path")?;
        let name = last.ident.to_string();
        if path.is_ident("Self")
            && let Some(current) = self.current
        {
            return Ok(Ty::Named(current));
        }
        if path.segments.len() == 1
            && let Some((_, ty)) = subst.iter().find(|(param, _)| *param == name)
        {
            return Ok(ty.clone());
        }
        let args = match &last.arguments {
            PathArguments::AngleBracketed(args) => args
                .args
                .iter()
                .filter_map(|arg| match arg {
                    GenericArgument::Type(ty) => Some(self.ty(module, ty, subst)),
                    _ => None,
                })
                .collect::<Result<Vec<_>, _>>()?,
            PathArguments::None => Vec::new(),
            PathArguments::Parenthesized(_) => return Err(format!("closure type {name}")),
        };
        let one = |args: &[Ty]| -> Result<Box<Ty>, String> {
            args.first()
                .cloned()
                .map(Box::new)
                .ok_or_else(|| format!("{name} needs a type argument"))
        };
        if let Some(prim) = primitive(&name) {
            return Ok(prim);
        }
        let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        // Prelude and std types, found by name whatever path reaches them.
        let found = self.index.resolve_path(module, &segments);
        let external = match &found {
            Ok(Found::External(path)) => Some(path.last().cloned().unwrap_or_default()),
            Err(_) => Some(name.clone()),
            Ok(Found::Item(_)) => None,
        };
        if let Some(std) = external {
            return Ok(match std.as_str() {
                "String" => Ty::Str(Str::Owned),
                "Option" => Ty::Option(one(&args)?),
                "Vec" => Ty::List(one(&args)?, List::Vec),
                "Result" => {
                    let err = args.get(1).cloned().ok_or("Result needs an error type")?;
                    Ty::Result(one(&args)?, Box::new(err))
                }
                "Box" => match *one(&args)? {
                    Ty::Dyn(name, _) => Ty::Handler(name, true),
                    Ty::Str(_) => Ty::Str(Str::Boxed),
                    Ty::List(item, _) => Ty::List(item, List::Boxed),
                    ty => Ty::Boxed(Box::new(ty)),
                },
                "Arc" | "Rc" => match *one(&args)? {
                    Ty::Dyn(name, _) if std == "Arc" => Ty::Handler(name, false),
                    Ty::Str(_) => Ty::Str(Str::Shared),
                    Ty::List(item, _) => Ty::List(item, List::Shared),
                    ty if std == "Arc" => Ty::Shared(Box::new(ty)),
                    ty => return Err(format!("{std}<{ty:?}>")),
                },
                "Cow" => match *one(&args)? {
                    Ty::Str(Str::Borrowed) => Ty::Str(Str::Cow),
                    ty => return Err(format!("Cow<{ty:?}>")),
                },
                "HashSet" | "BTreeSet" => Ty::List(
                    one(&args)?,
                    List::Set(if std == "HashSet" {
                        "std::collections::HashSet"
                    } else {
                        "std::collections::BTreeSet"
                    }),
                ),
                "str" => Ty::Str(Str::Borrowed),
                "HashMap" | "BTreeMap" => {
                    let value = args.get(1).cloned().ok_or("map needs a value type")?;
                    let map = if std == "HashMap" {
                        "std::collections::HashMap"
                    } else {
                        "std::collections::BTreeMap"
                    };
                    Ty::List(
                        Box::new(Ty::Tuple(vec![*one(&args)?, value])),
                        List::Map(map),
                    )
                }
                "Value" | "Map" => Ty::Named(self.json(&segments)?),
                "Duration" => Ty::Duration,
                _ => return Err(format!("unsupported type {}", segments.join("::"))),
            });
        }
        let Ok(Found::Item(id)) = found else {
            unreachable!()
        };
        self.item(id, &name, args)
    }

    /// A JSON value crossing as its serde form.
    fn json(&mut self, segments: &[String]) -> Result<usize, String> {
        let name = segments.last().unwrap();
        let rust = match name.as_str() {
            "Value" => "serde_json::Value",
            _ => "serde_json::Map<String, serde_json::Value>",
        };
        if let Some(id) = self.defs.iter().position(|def| def.rust == rust) {
            return Ok(id);
        }
        Ok(self.push(Def {
            projection: None,
            append: None,
            view: None,
            rust: rust.into(),
            lifetimes: 0,
            clone: true,
            borrowed: BorrowedShape::None,
        }))
    }

    fn push(&mut self, def: Def) -> usize {
        let id = self.defs.len();
        self.defs.push(def);
        id
    }

    /// The named item `id`, used with type arguments `args`.
    pub(super) fn item(&mut self, id: ItemId, name: &str, args: Vec<Ty>) -> Result<Ty, String> {
        let item = self.index.item(id).clone();
        if let Item::Type(alias) = &item {
            let params: Vec<String> = alias
                .generics
                .type_params()
                .map(|p| p.ident.to_string())
                .collect();
            let subst: Vec<(String, Ty)> = params.into_iter().zip(args).collect();
            return self.ty(id.module, &alias.ty, &subst);
        }
        if let Item::Trait(_) = item {
            return Err(format!("trait {name} used as a type"));
        }
        if let Some(&def) = self.by_item.get(&id) {
            return Ok(Ty::Named(def));
        }
        let rust = self.public_path(id, name);
        let lifetimes = generics(&item).lifetimes().count();
        let krate = self.index.modules[id.module].krate;
        let clone = derives(&item, "Clone")
            || self.index.modules.iter().filter(|m| m.krate == krate).flat_map(|m| &m.items).any(|other| {
                matches!(other, Item::Impl(i)
                    if i.trait_.as_ref().is_some_and(|(_, t, _)| t.segments.last().is_some_and(|s| s.ident == "Clone"))
                    && matches!(&*i.self_ty, syn::Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == name)))
            });
        // Reserve the slot first: a type may refer to itself.
        let def = self.push(Def {
            projection: None,
            append: None,
            view: None,
            rust,
            lifetimes,
            clone,
            borrowed: BorrowedShape::None,
        });
        self.by_item.insert(id, def);
        let outer = self.current.replace(def);
        let shape = if lifetimes == 0 {
            Ok(BorrowedShape::None)
        } else {
            self.borrowed_shape(id, &item)
        };
        self.current = outer;
        self.defs[def].borrowed = shape?;
        self.defs[def].append = super::append::detect(self, id, &item)?;
        self.defs[def].projection = self.projection(id, &item)?;
        self.defs[def].view = self.view(id, &item)?;
        Ok(Ty::Named(def))
    }

    /// Private readonly records opt in only through a type-owned typed constructor.
    fn projection(&mut self, id: ItemId, item: &Item) -> Result<Option<Vec<Field>>, String> {
        let Item::Struct(record) = item else {
            return Ok(None);
        };
        if record.generics.lifetimes().count() != 1 {
            return Ok(None);
        }
        let Fields::Named(fields) = &record.fields else {
            return Ok(None);
        };
        if fields
            .named
            .iter()
            .any(|f| !matches!(&f.vis, syn::Visibility::Restricted(v) if v.path.is_ident("crate")))
        {
            return Ok(None);
        }
        let constructor = self.index.modules.iter().filter(|m| m.krate == "mod_api").flat_map(|m| &m.items).find_map(|item| {
            let Item::Impl(i) = item else { return None; };
            if !matches!(&*i.self_ty, syn::Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == record.ident)) { return None; }
            i.items.iter().find_map(|item| {
                let syn::ImplItem::Fn(f) = item else { return None; };
                (f.sig.ident == "__wire_from_fields" && matches!(f.vis, syn::Visibility::Public(_)) && f.attrs.iter().any(|a| matches!(&a.meta, syn::Meta::List(m) if m.path.is_ident("doc") && m.tokens.to_string() == "hidden"))).then(|| f.sig.clone())
            })
        });
        let Some(constructor) = constructor else {
            return Ok(None);
        };
        if constructor.inputs.len() != fields.named.len()
            || !matches!(&constructor.output, syn::ReturnType::Type(_, t) if matches!(&**t, syn::Type::Path(p) if p.path.is_ident("Self")))
        {
            return Err(format!(
                "{}: invalid borrowed projection constructor",
                record.ident
            ));
        }
        fn read_only(types: &Types<'_>, ty: &Ty) -> bool {
            match ty {
                Ty::Ref(_, true) | Ty::Dyn(..) => false,
                Ty::Ref(t, false)
                | Ty::Option(t)
                | Ty::Array(t, _)
                | Ty::List(t, _)
                | Ty::Boxed(t)
                | Ty::Shared(t) => read_only(types, t),
                Ty::Tuple(v) => v.iter().all(|t| read_only(types, t)),
                Ty::Result(a, b) => read_only(types, a) && read_only(types, b),
                Ty::Named(d) => types.defs[*d].lifetimes == 0,
                _ => true,
            }
        }
        let described = self.fields(id.module, fields.named.iter())?;
        for (field, argument) in described.iter().zip(&constructor.inputs) {
            let syn::FnArg::Typed(argument) = argument else {
                return Ok(None);
            };
            if !matches!(&*argument.pat, syn::Pat::Ident(p) if p.ident == field.rust)
                || self.ty(id.module, &argument.ty, &[])? != field.ty
            {
                return Err(format!(
                    "{}: projection constructor does not match fields",
                    record.ident
                ));
            }
            if !read_only(self, &field.ty) {
                return Ok(None);
            }
        }
        Ok(Some(described))
    }
    /// Recover a field-preserving snapshot conversion without constructing its owned result.
    /// Restrict discovery to unqualified types defined in the same module: name matches
    /// across modules or through imports are not proof of type identity.
    pub(super) fn snapshot_projection(&self, view: &View) -> Option<Vec<String>> {
        let (Ty::Named(source), Ty::Named(target)) = (&view.pointee, &view.snapshot) else {
            return None;
        };
        let source_id = self.by_item.iter().find(|(_, d)| **d == *source)?.0;
        let target_id = self.by_item.iter().find(|(_, d)| **d == *target)?.0;
        if source_id.module != target_id.module {
            return None;
        }
        let Item::Struct(source_record) = self.index.item(*source_id) else {
            return None;
        };
        let Item::Struct(target_record) = self.index.item(*target_id) else {
            return None;
        };
        let Fields::Named(fields) = &target_record.fields else {
            return None;
        };
        for item in &self.index.modules[source_id.module].items {
            let Item::Impl(i) = item else {
                continue;
            };
            let syn::Type::Path(to) = &*i.self_ty else {
                continue;
            };
            if !to.path.is_ident(&target_record.ident) {
                continue;
            }
            let Some((_, from, _)) = &i.trait_ else {
                continue;
            };
            if from.segments.len() != 1 || from.segments[0].ident != "From" {
                continue;
            }
            let syn::PathArguments::AngleBracketed(args) = &from.segments[0].arguments else {
                continue;
            };
            let Some(syn::GenericArgument::Type(syn::Type::Reference(reference))) =
                args.args.first()
            else {
                continue;
            };
            let syn::Type::Path(from_type) = &*reference.elem else {
                continue;
            };
            if reference.mutability.is_some() || !from_type.path.is_ident(&source_record.ident) {
                continue;
            }
            let Some(f) = i.items.iter().find_map(|item| match item {
                syn::ImplItem::Fn(f) if f.sig.ident == "from" => Some(f),
                _ => None,
            }) else {
                continue;
            };
            let Some(syn::FnArg::Typed(arg)) = f.sig.inputs.first() else {
                continue;
            };
            let syn::Pat::Ident(binding) = &*arg.pat else {
                continue;
            };
            let [syn::Stmt::Expr(syn::Expr::Struct(result), None)] = &f.block.stmts[..] else {
                continue;
            };
            if !result.path.is_ident("Self")
                || result.rest.is_some()
                || result.fields.len() != fields.named.len()
            {
                continue;
            }
            let projection = fields.named
                .iter()
                .map(|field| {
                    let value = result
                        .fields
                        .iter()
                        .find(|v| matches!(&v.member, syn::Member::Named(n) if Some(n) == field.ident.as_ref()))?;
                    let expr = match &value.expr {
                        syn::Expr::MethodCall(call)
                            if call.method == "clone"
                                && call.args.is_empty()
                                && call.turbofish.is_none() =>
                        {
                            &*call.receiver
                        }
                        expr => expr,
                    };
                    let syn::Expr::Field(access) = expr else {
                        return None;
                    };
                    let syn::Expr::Path(base) = &*access.base else {
                        return None;
                    };
                    if !base.path.is_ident(&binding.ident) {
                        return None;
                    }
                    let syn::Member::Named(member) = &access.member else {
                        return None;
                    };
                    Some(member.to_string())
                })
                .collect::<Option<Vec<_>>>();
            if projection.is_some() {
                return projection;
            }
        }
        None
    }

    fn view(&mut self, id: ItemId, item: &Item) -> Result<Option<View>, String> {
        let Item::Struct(record) = item else {
            return Ok(None);
        };
        if record.generics.lifetimes().count() == 0 {
            return Ok(None);
        }
        let Fields::Named(fields) = &record.fields else {
            return Ok(None);
        };
        let [field] = fields.named.iter().collect::<Vec<_>>()[..] else {
            return Ok(None);
        };
        let syn::Type::Reference(reference) = &field.ty else {
            return Ok(None);
        };
        let name = &record.ident;
        let items = self.index.modules[id.module].items.clone();
        let ctor = items.iter().filter_map(|item| match item {
            Item::Impl(i) if matches!(&*i.self_ty, syn::Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == *name)) => Some(i),
            _ => None,
        }).flat_map(|i| &i.items).any(|item| matches!(item,syn::ImplItem::Fn(f) if f.sig.ident == "new" && f.sig.inputs.len() == 1));
        if !ctor {
            return Ok(None);
        }
        let pointee = self.ty(id.module, &reference.elem, &[])?;
        let Ty::Named(d) = &pointee else {
            return Ok(None);
        };
        let target = self.defs[*d].rust.split("::").last().unwrap().to_string();
        let snapshot_arg = self.index.modules.iter().flat_map(|m| m.items.iter().map(move |i| (m,i))).find_map(|(module,item)| {
            let Item::Impl(i) = item else { return None; };
            if !matches!(&*i.self_ty,syn::Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == target)) { return None; }
            i.items.iter().find_map(|item| {
                let syn::ImplItem::Fn(f) = item else { return None; };
                if f.sig.ident != "apply_snapshot" || f.sig.inputs.len() != 2 { return None; }
                let syn::FnArg::Typed(arg) = f.sig.inputs.last()? else { return None; };
                Some((module.path.clone(),(*arg.ty).clone()))
            })
        });
        let Some((module_path, arg)) = snapshot_arg else {
            return Ok(None);
        };
        let module = self
            .index
            .modules
            .iter()
            .position(|m| m.path == module_path)
            .unwrap();
        let snapshot = self.ty(module, &arg, &[])?;
        Ok(Some(View {
            field: field.ident.as_ref().unwrap().to_string(),
            pointee,
            mutable: reference.mutability.is_some(),
            snapshot,
        }))
    }

    /// Only borrowed call arguments need structural descriptions. Owned values are
    /// opaque to the generator and cross through their explicit Wire implementations.
    fn borrowed_shape(&mut self, id: ItemId, item: &Item) -> Result<BorrowedShape, String> {
        match item {
            Item::Struct(record) => match &record.fields {
                Fields::Named(fields)
                    if fields
                        .named
                        .iter()
                        .all(|f| matches!(f.vis, syn::Visibility::Public(_))) =>
                {
                    self.fields(id.module, fields.named.iter())
                        .map(BorrowedShape::Record)
                }
                Fields::Unnamed(fields)
                    if fields
                        .unnamed
                        .iter()
                        .all(|f| matches!(f.vis, syn::Visibility::Public(_))) =>
                {
                    fields
                        .unnamed
                        .iter()
                        .map(|f| self.ty(id.module, &f.ty, &[]))
                        .collect::<Result<_, _>>()
                        .map(BorrowedShape::TupleRecord)
                }
                _ => Ok(BorrowedShape::None),
            },
            Item::Enum(record) => record
                .variants
                .iter()
                .map(|case| {
                    let payload = match &case.fields {
                        Fields::Unit => Payload::None,
                        Fields::Unnamed(fields) => Payload::Tuple(
                            fields
                                .unnamed
                                .iter()
                                .map(|f| self.ty(id.module, &f.ty, &[]))
                                .collect::<Result<_, _>>()?,
                        ),
                        Fields::Named(fields) => {
                            Payload::Record(self.fields(id.module, fields.named.iter())?)
                        }
                    };
                    Ok(Case {
                        rust: case.ident.to_string(),
                        payload,
                    })
                })
                .collect::<Result<_, String>>()
                .map(BorrowedShape::Variant),
            _ => Ok(BorrowedShape::None),
        }
    }

    fn fields<'f>(
        &mut self,
        module: usize,
        fields: impl Iterator<Item = &'f syn::Field>,
    ) -> Result<Vec<Field>, String> {
        fields
            .map(|f| {
                let rust = f.ident.as_ref().unwrap().to_string();
                Ok(Field {
                    ty: self.ty(module, &f.ty, &[])?,
                    rust,
                })
            })
            .collect()
    }

    /// The path generated glue uses for item `id`: `mod_api::Name` when `mod_api`'s root
    /// exports it, its defining module's path otherwise.
    fn public_path(&mut self, id: ItemId, name: &str) -> String {
        if let Ok(Found::Item(root)) = self.index.resolve(self.api_root, name)
            && root == id
        {
            return format!("mod_api::{name}");
        }
        let module = &self.index.modules[id.module];
        let mut path = vec![module.krate.to_string()];
        path.extend(module.path.iter().cloned());
        path.push(name.to_string());
        path.join("::")
    }
}

fn primitive(name: &str) -> Option<Ty> {
    Some(match name {
        "bool" => Ty::Prim("bool"),
        "u8" => Ty::Prim("u8"),
        "u16" => Ty::Prim("u16"),
        "u32" => Ty::Prim("u32"),
        "u64" => Ty::Prim("u64"),
        "usize" => Ty::Prim("usize"),
        "i8" => Ty::Prim("i8"),
        "i16" => Ty::Prim("i16"),
        "i32" => Ty::Prim("i32"),
        "i64" => Ty::Prim("i64"),
        "isize" => Ty::Prim("isize"),
        "f32" => Ty::Prim("f32"),
        "f64" => Ty::Prim("f64"),
        "char" => Ty::Prim("char"),
        "u128" => Ty::Wide("u128"),
        "i128" => Ty::Wide("i128"),
        _ => return None,
    })
}

/// A trait object's trait as the generator names it: its last path segment, with its type
/// arguments as written (`ResourceHandler<ItemResource>`).
pub(super) fn trait_name(object: &syn::TypeTraitObject) -> Result<String, String> {
    object
        .bounds
        .iter()
        .find_map(|bound| match bound {
            syn::TypeParamBound::Trait(bound) => bound.path.segments.last().map(|s| {
                let args = match &s.arguments {
                    PathArguments::AngleBracketed(args) => format!(
                        "<{}>",
                        args.args.iter().map(written).collect::<Vec<_>>().join(", ")
                    ),
                    _ => String::new(),
                };
                format!("{}{args}", s.ident)
            }),
            _ => None,
        })
        .ok_or_else(|| format!("dyn {}", object.to_token_stream()))
}

/// Tokens as written, without the spaces `quote` puts around punctuation
/// (`Option<Direction>`, `dyn ResourceHandler<ItemResource>`).
pub(super) fn written(tokens: &impl ToTokens) -> String {
    tokens
        .to_token_stream()
        .to_string()
        .replace(" < ", "<")
        .replace("< ", "<")
        .replace(" <", "<")
        .replace(" >", ">")
        .replace(" ,", ",")
        .replace("( )", "()")
}

fn generics(item: &Item) -> syn::Generics {
    match item {
        Item::Struct(item) => item.generics.clone(),
        Item::Enum(item) => item.generics.clone(),
        Item::Type(item) => item.generics.clone(),
        _ => syn::Generics::default(),
    }
}

fn derives(item: &Item, name: &str) -> bool {
    let attrs = match item {
        Item::Struct(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        _ => return false,
    };
    attrs.iter().any(|attr| {
        attr.path().is_ident("derive")
            && attr
                .to_token_stream()
                .to_string()
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|word| word == name)
    })
}
