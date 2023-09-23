use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::fmt::{Formatter, Write};

use indenter::CodeFormatter;
use itertools::Itertools;

use crate::parser;

type Key = String;
type Field = String;
type Lang = String;

pub struct CodeGenerator {
    by_language: BTreeMap<Lang, BTreeMap<Key, Category>>,
    field_metadata: BTreeMap<Key, BTreeMap<Field, Meta>>,
    normalized_langs: BTreeMap<Lang, String>,
}

enum Category {
    Link(String, String),
    Fields(BTreeMap<Field, Value>),
}

#[derive(Clone)]
enum Value {
    Empty,
    Literal(String),
    Array(Vec<String>),
    Array2d(Vec<Vec<String>>),
}

struct TypeFormatter<'a> {
    meta: &'a Meta,
}

impl<'a> TypeFormatter<'a> {
    fn new(meta: &'a Meta) -> Self {
        Self { meta }
    }
}

impl<'a> std::fmt::Display for TypeFormatter<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.meta.ty {
            None => unreachable!(),
            Some(ty) => {
                if self.meta.optional {
                    match self.meta.container_ty {
                        ContainerType::Singleton => write!(f, "Option<{}>", ty),
                        ContainerType::Array => write!(f, "Option<&[{}]>", ty),
                        ContainerType::Array2D => write!(f, "Option<&[&[{}]]>", ty),
                    }
                } else {
                    match self.meta.container_ty {
                        ContainerType::Singleton => write!(f, "{}", ty),
                        ContainerType::Array => write!(f, "&[{}]", ty),
                        ContainerType::Array2D => write!(f, "&[&[{}]]", ty),
                    }
                }
            }
        }
    }
}

struct ValueFormatter<'a> {
    value: &'a Value,
    meta: &'a Meta,
}

impl<'a> ValueFormatter<'a> {
    fn new(value: &'a Value, meta: &'a Meta) -> Self {
        Self { value, meta }
    }

    fn format(f: &mut Formatter<'_>, value: &Value, ty: &Type) -> std::fmt::Result {
        match value {
            Value::Empty => unreachable!(),
            Value::Literal(x) => write!(f, "{}", LiteralFormatter::new(x, ty),),
            Value::Array(x) => write!(
                f,
                "&[{val}]",
                val = x
                    .iter()
                    .map(|x| format!("{}", LiteralFormatter::new(x, ty)))
                    .join(", "),
            ),
            Value::Array2d(x) => {
                write!(f, r#"&["#,)?;

                for values in x.iter() {
                    write!(
                        f,
                        r#"
                &[{}],"#,
                        values
                            .iter()
                            .map(|x| format!("{}", LiteralFormatter::new(x, ty)))
                            .join(", "),
                    )?;
                }

                write!(
                    f,
                    r#"
            ]"#,
                )?;

                Ok(())
            }
        }
    }
}

impl<'a> std::fmt::Display for ValueFormatter<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.meta.ty {
            None => unreachable!(),
            Some(ty) => {
                if self.meta.optional {
                    match self.value {
                        Value::Empty => write!(f, "None"),
                        _ => {
                            write!(f, "Some(")?;
                            Self::format(f, self.value, ty)?;
                            write!(f, ")")
                        }
                    }
                } else {
                    match self.value {
                        Value::Empty => unreachable!(),
                        _ => Self::format(f, self.value, ty),
                    }
                }
            }
        }
    }
}

struct LiteralFormatter<'a> {
    value: &'a str,
    ty: &'a Type,
}

impl<'a> LiteralFormatter<'a> {
    fn new(value: &'a str, ty: &'a Type) -> Self {
        Self { value, ty }
    }
}

impl<'a> std::fmt::Display for LiteralFormatter<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.ty {
            Type::String => write!(f, "{:?}", self.value),
            Type::Integer => write!(f, "{}", self.value),
        }
    }
}

impl Value {
    fn with_fixed_type<'a>(&'a self, meta: &Meta) -> Cow<'a, Self> {
        match meta.container_ty {
            ContainerType::Singleton => match self {
                Value::Empty | Value::Literal(_) => Cow::Borrowed(self),
                Value::Array(_) => unreachable!(),
                Value::Array2d(_) => unreachable!(),
            },
            ContainerType::Array => match self {
                Value::Empty => Cow::Borrowed(self),
                Value::Literal(x) => Cow::Owned(Value::Array(vec![x.clone()])),
                Value::Array(_) => Cow::Borrowed(self),
                Value::Array2d(_) => unreachable!(),
            },
            ContainerType::Array2D => match self {
                Value::Empty => Cow::Borrowed(self),
                Value::Literal(x) => Cow::Owned(Self::Array2d(vec![vec![x.clone()]])),
                Value::Array(x) => Cow::Owned(Self::Array2d(vec![x.clone()])),
                Value::Array2d(_) => Cow::Borrowed(self),
            },
        }
    }

    fn generate<W: Write>(
        &self,
        field_name: &str,
        meta: &Meta,
        f: &mut CodeFormatter<W>,
    ) -> std::fmt::Result {
        let ty = meta.ty.as_ref().unwrap();
        let type_formatter = TypeFormatter::new(meta);
        let value_formatter = ValueFormatter::new(self, meta);

        match self {
            Value::Array2d(x) => {
                write!(
                    f,
                    r#"
                    /// ```ignore
                    /// &[
                    "#,
                )?;

                for values in x.iter() {
                    write!(
                        f,
                        r#"
                        ///     &[{}],
                        "#,
                        values
                            .iter()
                            .map(|x| format!("{}", LiteralFormatter::new(x, ty)))
                            .join(", "),
                    )?;
                }

                write!(
                    f,
                    r#"
                    /// ]
                    /// ```
                    "#,
                )?;
            }
            _ => {
                write!(
                    f,
                    r#"
                    /// `{val}`
                    "#,
                    val = value_formatter,
                )?;
            }
        }

        write!(
            f,
            r#"
            pub const {key}: {ty} = {val};
            "#,
            key = field_name,
            ty = type_formatter,
            val = value_formatter,
        )?;

        Ok(())
    }
}

impl CodeGenerator {
    pub fn new(objects: HashMap<String, Vec<parser::Object>>) -> Self {
        let mut by_language = BTreeMap::<Lang, BTreeMap<Key, Category>>::new();
        let mut field_metadata = BTreeMap::<Key, BTreeMap<Field, Meta>>::new();
        let mut normalized_langs = BTreeMap::<Lang, String>::new();

        for (lang, objects) in objects.iter() {
            normalized_langs.insert(lang.to_string(), lang.replace("@", "_"));

            let lang_categories = by_language
                .entry(lang.to_string())
                .or_insert(BTreeMap::new());

            for object in objects.iter() {
                if object.name == "LC_COLLATE"
                    || object.name == "LC_CTYPE"
                    || object.name == "LC_MEASUREMENT"
                    || object.name == "LC_PAPER"
                    || object.name == "LC_NAME"
                {
                    continue;
                } else if object.values.len() == 1 {
                    let (key, value) = &object.values[0];
                    #[allow(clippy::single_match)]
                    match key.as_str() {
                        "copy" => {
                            assert_eq!(value.len(), 1);

                            match &value[0] {
                                parser::Value::String(x) => {
                                    lang_categories.insert(
                                        object.name.clone(),
                                        Category::Link(x.replace("@", "_"), object.name.clone()),
                                    );
                                }
                                x => panic!("unexpected value for key {}: {:?}", key, x),
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                let mut fields = BTreeMap::<Field, Value>::new();

                let cat_field_meta = field_metadata
                    .entry(object.name.clone())
                    .or_insert(BTreeMap::new());

                for (key, group) in &object
                    .values
                    .iter()
                    .filter(|x| !x.1.is_empty())
                    .sorted_by(|a, b| Ord::cmp(&a.0, &b.0))
                    .group_by(|x| x.0.clone())
                {
                    let key = key
                        .replace("'", "")
                        .replace("\"", "")
                        .replace("-", "_")
                        .replace("=", "eq")
                        .replace("<", "lt")
                        .replace("..", "dotdot")
                        .replace("2", "two")
                        .to_uppercase();
                    let group: Vec<_> = group.map(|x| &x.1).collect();

                    let meta = cat_field_meta.entry(key.clone()).or_insert(Meta::new());

                    if group.len() == 1 && group[0].is_empty() {
                        meta.make_optional();
                        fields.insert(key, Value::Empty);
                    } else if group.len() == 1 && group[0].len() == 1 {
                        let singleton = &group[0][0];

                        match singleton {
                            parser::Value::Raw(_) | parser::Value::String(_) => meta.mark_str(),
                            parser::Value::Integer(_) => meta.mark_int(),
                        }

                        fields.insert(key, Value::Literal(singleton.to_string()));
                    } else if group.len() == 1 && group[0].iter().map(u8::from).all_equal() {
                        let values = &group[0];
                        let vec = values.iter().map(|x| x.to_string()).collect::<Vec<_>>();

                        meta.mark_array();

                        match &values[0] {
                            parser::Value::Raw(_) | parser::Value::String(_) => meta.mark_str(),
                            parser::Value::Integer(_) => meta.mark_int(),
                        }

                        fields.insert(key, Value::Array(vec));
                    } else if group
                        .iter()
                        .map(|x| x.iter().map(u8::from))
                        .flatten()
                        .all_equal()
                    {
                        meta.mark_array_2d();

                        let mut vec = Vec::with_capacity(group.len());

                        for a in group.iter() {
                            for value in a.iter() {
                                match value {
                                    parser::Value::Raw(_) | parser::Value::String(_) => {
                                        meta.mark_str()
                                    }
                                    parser::Value::Integer(_) => meta.mark_int(),
                                }
                            }

                            let inner_vec = a.iter().map(|x| x.to_string()).collect::<Vec<_>>();

                            vec.push(inner_vec);
                        }

                        fields.insert(key, Value::Array2d(vec));
                    } else {
                        unimplemented!()
                    }
                }

                lang_categories.insert(object.name.clone(), Category::Fields(fields));
            }
        }

        for (_lang, categories) in by_language.iter_mut() {
            for (category_name, all_fields) in field_metadata.iter_mut() {
                let language_cats = categories
                    .entry(category_name.clone())
                    .or_insert(Category::Fields(BTreeMap::new()));

                match language_cats {
                    Category::Link(_, _) => {}
                    Category::Fields(fields) => {
                        for (field, meta) in all_fields {
                            if let None = fields.get(field) {
                                fields.insert(field.clone(), Value::Empty);
                                meta.make_optional();
                            }
                        }
                    }
                }
            }
        }

        Self {
            by_language,
            field_metadata,
            normalized_langs,
        }
    }

    fn generate<W: Write>(&self, f: &mut CodeFormatter<W>) -> std::fmt::Result {
        write!(
            f,
            r#"
            #![no_std]

            #[derive(Debug)]
            pub struct UnknownLocale;

            "#,
        )?;

        for (lang, categories) in self.by_language.iter() {
            let lang = &self.normalized_langs[lang];

            write!(
                f,
                r#"

                #[allow(non_snake_case,non_camel_case_types,dead_code,unused_imports)]
                pub mod {} {{
                "#,
                lang,
            )?;
            f.indent(1);

            for (category_name, category) in categories.iter() {
                let category_metadata = self.field_metadata.get(category_name).unwrap();

                match category {
                    Category::Link(lang, category_name) => {
                        write!(
                            f,
                            r#"
                            pub use super::{}::{};
                            "#,
                            lang, category_name,
                        )?;
                    }
                    Category::Fields(fields) => {
                        write!(
                            f,
                            r#"
                            pub mod {} {{
                            "#,
                            category_name,
                        )?;

                        f.indent(1);

                        for (field_name, meta) in category_metadata.iter() {
                            fields
                                .get(field_name)
                                .unwrap()
                                .with_fixed_type(meta)
                                .generate(field_name, meta, f)?;
                        }

                        f.dedent(1);

                        write!(
                            f,
                            r#"
                            }}
                            "#,
                        )?;
                    }
                }
            }

            f.dedent(1);

            write!(
                f,
                r#"
                }}
                "#,
            )?
        }

        self.generate_variants(f)?;

        Ok(())
    }

    fn generate_variants<W: Write>(&self, f: &mut CodeFormatter<W>) -> std::fmt::Result {
        write!(
            f,
            r#"

            #[allow(non_camel_case_types,dead_code)]
            #[derive(Debug, Copy, Clone, PartialEq)]
            pub enum Locale {{
            "#,
        )?;
        f.indent(1);

        for (lang, norm) in self.normalized_langs.iter() {
            let desc = match self
                .by_language
                .get(lang)
                .and_then(|l| l.get("LC_IDENTIFICATION"))
            {
                Some(Category::Fields(fields)) => match fields.get("TITLE") {
                    Some(Value::Literal(title)) => {
                        let mut title = title.clone();
                        if !title.ends_with('.') {
                            title.push('.');
                        }
                        title
                    }
                    _ => match lang == "POSIX" {
                        true => "POSIX Standard Locale.".to_string(),
                        false => "".to_string(),
                    },
                },
                _ => "".to_string(),
            };
            write!(
                f,
                r#"
                /// `{lang}`: {desc}
                {norm},
                "#,
                lang = lang,
                desc = desc,
                norm = norm,
            )?;
        }

        f.dedent(1);
        write!(
            f,
            r#"
            }}

            impl core::str::FromStr for Locale {{
                type Err = UnknownLocale;

                fn from_str(s: &str) -> Result<Self, Self::Err> {{
                    core::convert::TryFrom::<&str>::try_from(s)
                }}
            }}

            impl core::convert::TryFrom<&str> for Locale {{
                type Error = UnknownLocale;

                fn try_from(i: &str) -> Result<Self, Self::Error> {{
                    match i {{
            "#,
        )?;
        f.indent(3);

        for (lang, norm) in self.normalized_langs.iter() {
            write!(
                f,
                r#"
                {lang:?} => Ok(Locale::{norm}),
                "#,
                lang = lang,
                norm = norm,
            )?;
        }

        f.dedent(3);
        write!(
            f,
            r#"
                        _ => Err(UnknownLocale),
                    }}
                }}
            }}

            #[macro_export]
            macro_rules! locale_match {{
                ($locale:expr => $($item:ident)::+) => {{{{
                    match $locale {{
            "#,
        )?;
        f.indent(3);

        for (_, norm) in self.normalized_langs.iter() {
            write!(
                f,
                r#"
                $crate::Locale::{norm} => $crate::{norm}::$($item)::+,
                "#,
                norm = norm,
            )?;
        }
        f.dedent(3);

        write!(
            f,
            r#"
                    }}
                }}}}
            }}

            "#,
        )
    }
}

impl std::fmt::Display for CodeGenerator {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut f = CodeFormatter::new(f, "    ");
        self.generate(&mut f)
    }
}

struct Meta {
    optional: bool,
    container_ty: ContainerType,
    ty: Option<Type>,
}

impl Meta {
    fn new() -> Self {
        Self {
            optional: false,
            container_ty: ContainerType::Singleton,
            ty: None,
        }
    }

    fn mark_str(&mut self) {
        self.ty = match self.ty {
            Some(Type::Integer) => Some(Type::String),
            Some(Type::String) => Some(Type::String),
            None => Some(Type::String),
        }
    }

    fn mark_int(&mut self) {
        self.ty = match self.ty {
            Some(Type::Integer) => Some(Type::Integer),
            Some(Type::String) => Some(Type::String),
            None => Some(Type::Integer),
        }
    }

    fn make_optional(&mut self) {
        self.optional = true;
    }

    fn mark_array(&mut self) {
        self.container_ty = self.container_ty.into_array();
    }

    fn mark_array_2d(&mut self) {
        self.container_ty = self.container_ty.into_array_2d();
    }
}

#[derive(Copy, Clone)]
pub enum ContainerType {
    Singleton,
    Array,
    Array2D,
}

pub enum Type {
    String,
    Integer,
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::String => f.write_str("&str"),
            Type::Integer => f.write_str("i64"),
        }
    }
}

impl ContainerType {
    fn into_array(self) -> Self {
        match self {
            Self::Singleton => Self::Array,
            _ => self,
        }
    }

    fn into_array_2d(self) -> Self {
        match self {
            Self::Singleton => Self::Array2D,
            Self::Array => Self::Array2D,
            _ => self,
        }
    }
}
