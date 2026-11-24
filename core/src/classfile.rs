//! Reads the names a compiled class uses: the classes, fields and methods it refers to, and its
//! mixin annotations. Only the constant pool and annotations are read; method bodies are skipped,
//! so nothing here decompiles or keeps the mod's code.

/// A class, field or method the class refers to. Class names use dots (`net.minecraft.client.Minecraft`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ref {
    pub class: String,
    pub member: Option<String>,
}

/// What a mixin class targets: its classes, and the methods its injectors name.
#[derive(Clone, Debug, Default)]
pub struct MixinInfo {
    pub targets: Vec<String>,
    pub methods: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ClassInfo {
    pub name: String,
    pub refs: Vec<Ref>,
    /// Set when the class carries `@Mixin`.
    pub mixin: Option<MixinInfo>,
}

#[derive(Debug)]
pub struct ParseError(pub &'static str);

enum Const {
    Utf8(String),
    Class(u16),
    MemberRef(u16, u16),
    /// A name and its descriptor; only the name is read.
    NameAndType(u16),
    Other,
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], ParseError> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.data.len()).ok_or(ParseError("class file ends early"))?;
        let out = &self.data[self.at..end];
        self.at = end;
        Ok(out)
    }
    fn u1(&mut self) -> Result<u8, ParseError> {
        Ok(self.bytes(1)?[0])
    }
    fn u2(&mut self) -> Result<u16, ParseError> {
        let b = self.bytes(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u4(&mut self) -> Result<u32, ParseError> {
        let b = self.bytes(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// Annotation element values, as far as mixins need them.
enum Value {
    Str(String),
    Class(String),
    Array(Vec<Value>),
    Other,
}

struct Annotation {
    ty: String,
    values: Vec<(String, Value)>,
}

impl Annotation {
    fn strings(&self, key: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (k, v) in &self.values {
            if k == key {
                flatten(v, &mut out);
            }
        }
        out
    }
}

fn flatten(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Str(s) | Value::Class(s) => out.push(s.clone()),
        Value::Array(items) => items.iter().for_each(|i| flatten(i, out)),
        Value::Other => {}
    }
}

const MIXIN: &str = "Lorg/spongepowered/asm/mixin/Mixin;";

/// The injector and accessor annotations whose `method` (or `value`) names target methods.
fn names_target_method(ty: &str) -> Option<&'static str> {
    let simple = ty.rsplit('/').next()?.trim_end_matches(';');
    match simple {
        "Inject" | "Redirect" | "ModifyArg" | "ModifyArgs" | "ModifyVariable" | "ModifyConstant" | "WrapOperation"
        | "WrapWithCondition" | "ModifyExpressionValue" | "ModifyReturnValue" | "ModifyReceiver" | "WrapMethod" => {
            Some("method")
        }
        _ => None,
    }
}

pub fn parse(data: &[u8]) -> Result<ClassInfo, ParseError> {
    let mut r = Reader { data, at: 0 };
    if r.u4()? != 0xCAFE_BABE {
        return Err(ParseError("not a class file"));
    }
    r.u2()?;
    r.u2()?;
    let count = r.u2()? as usize;
    let mut pool: Vec<Const> = Vec::with_capacity(count);
    pool.push(Const::Other);
    while pool.len() < count {
        let tag = r.u1()?;
        let c = match tag {
            1 => {
                let len = r.u2()? as usize;
                Const::Utf8(String::from_utf8_lossy(r.bytes(len)?).into_owned())
            }
            3 | 4 => {
                r.u4()?;
                Const::Other
            }
            5 | 6 => {
                r.bytes(8)?;
                // Longs and doubles take two slots.
                pool.push(Const::Other);
                Const::Other
            }
            7 => Const::Class(r.u2()?),
            8 => {
                r.u2()?;
                Const::Other
            }
            9..=11 => Const::MemberRef(r.u2()?, r.u2()?),
            12 => {
                let name = r.u2()?;
                r.u2()?;
                Const::NameAndType(name)
            }
            15 => {
                r.u1()?;
                r.u2()?;
                Const::Other
            }
            16 | 19 | 20 => {
                r.u2()?;
                Const::Other
            }
            17 | 18 => {
                r.u2()?;
                r.u2()?;
                Const::Other
            }
            _ => return Err(ParseError("unknown constant pool tag")),
        };
        pool.push(c);
    }
    let utf8 = |i: u16| match pool.get(i as usize) {
        Some(Const::Utf8(s)) => Some(s.as_str()),
        _ => None,
    };
    let class_name = |i: u16| match pool.get(i as usize) {
        Some(Const::Class(n)) => utf8(*n).map(|s| s.replace('/', ".")),
        _ => None,
    };

    let mut info = ClassInfo::default();
    for c in &pool {
        match c {
            Const::Class(n) => {
                // Array classes (`[Lnet/...;`) name their element type.
                if let Some(s) = utf8(*n) {
                    let s = s.trim_start_matches('[');
                    let s = s.strip_prefix('L').and_then(|s| s.strip_suffix(';')).unwrap_or(s);
                    if !s.is_empty() && s.len() > 1 {
                        info.refs.push(Ref { class: s.replace('/', "."), member: None });
                    }
                }
            }
            Const::MemberRef(class, nat) => {
                let (Some(class), Some(Const::NameAndType(name))) = (class_name(*class), pool.get(*nat as usize)) else {
                    continue;
                };
                if let Some(name) = utf8(*name) {
                    info.refs.push(Ref { class, member: Some(name.to_string()) });
                }
            }
            Const::Utf8(_) | Const::NameAndType(_) | Const::Other => {}
        }
    }

    r.u2()?;
    let this = r.u2()?;
    info.name = class_name(this).unwrap_or_default();
    r.u2()?;
    let interfaces = r.u2()?;
    r.bytes(interfaces as usize * 2)?;

    let mut injected = Vec::new();
    for _ in 0..2 {
        // Fields, then methods: only their annotations matter.
        let n = r.u2()?;
        for _ in 0..n {
            r.bytes(6)?;
            for a in attributes(&mut r, &utf8)? {
                if let Some(key) = names_target_method(&a.ty) {
                    injected.extend(a.strings(key));
                }
            }
        }
    }
    for a in attributes(&mut r, &utf8)? {
        if a.ty == MIXIN {
            let mut targets: Vec<String> = a.strings("value");
            targets.extend(a.strings("targets"));
            let targets = targets
                .into_iter()
                .map(|t| t.strip_prefix('L').and_then(|t| t.strip_suffix(';')).unwrap_or(&t).replace('/', "."))
                .collect();
            info.mixin = Some(MixinInfo { targets, methods: Vec::new() });
        }
    }
    if let Some(m) = &mut info.mixin {
        // `render(Lnet/...;)V`, or `Lnet/minecraft/Owner;render(...)V`: keep the method's name.
        m.methods = injected
            .iter()
            .map(|s| {
                let s = s.rsplit_once(';').filter(|(owner, _)| owner.starts_with('L')).map_or(s.as_str(), |(_, rest)| rest);
                s.split(['(', ':']).next().unwrap_or(s).to_string()
            })
            .filter(|s| !s.is_empty())
            .collect();
        m.methods.sort();
        m.methods.dedup();
    }
    Ok(info)
}

/// Reads an attribute table; returns the annotations among its invisible and visible annotation attributes.
fn attributes<'a>(r: &mut Reader<'a>, utf8: &dyn Fn(u16) -> Option<&'a str>) -> Result<Vec<Annotation>, ParseError> {
    let mut out = Vec::new();
    let n = r.u2()?;
    for _ in 0..n {
        let name = r.u2()?;
        let len = r.u4()? as usize;
        let body = r.bytes(len)?;
        if matches!(utf8(name), Some("RuntimeInvisibleAnnotations" | "RuntimeVisibleAnnotations")) {
            let mut a = Reader { data: body, at: 0 };
            let count = a.u2()?;
            for _ in 0..count {
                out.push(annotation(&mut a, utf8)?);
            }
        }
    }
    Ok(out)
}

fn annotation<'a>(r: &mut Reader<'a>, utf8: &dyn Fn(u16) -> Option<&'a str>) -> Result<Annotation, ParseError> {
    let ty = utf8(r.u2()?).unwrap_or_default().to_string();
    let pairs = r.u2()?;
    let mut values = Vec::new();
    for _ in 0..pairs {
        let key = utf8(r.u2()?).unwrap_or_default().to_string();
        values.push((key, value(r, utf8)?));
    }
    Ok(Annotation { ty, values })
}

fn value<'a>(r: &mut Reader<'a>, utf8: &dyn Fn(u16) -> Option<&'a str>) -> Result<Value, ParseError> {
    Ok(match r.u1()? {
        b's' => Value::Str(utf8(r.u2()?).unwrap_or_default().to_string()),
        b'c' => Value::Class(utf8(r.u2()?).unwrap_or_default().to_string()),
        b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z' => {
            r.u2()?;
            Value::Other
        }
        b'e' => {
            r.u2()?;
            r.u2()?;
            Value::Other
        }
        b'@' => {
            annotation(r, utf8)?;
            Value::Other
        }
        b'[' => {
            let n = r.u2()?;
            let mut items = Vec::with_capacity(n as usize);
            for _ in 0..n {
                items.push(value(r, utf8)?);
            }
            Value::Array(items)
        }
        _ => return Err(ParseError("unknown annotation value")),
    })
}
