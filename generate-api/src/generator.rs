use crate::parser;
use itertools::Itertools;
use std::collections::HashMap;
use std::fmt;

fn generate_object(
    f: &mut fmt::Formatter,
    object: &parser::Object,
    locales: &HashMap<String, Vec<parser::Object>>,
) -> fmt::Result {
    for (key, group) in &object
        .values
        .iter()
        .filter(|x| x.1.len() > 0)
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

        if key == "copy" || key == "include" {
            match &group[0][0] {
                parser::Value::String(other_lang) => {
                    let other = locales
                        .get(other_lang)
                        .expect(&format!("unknown locale: {}", other_lang));
                    let other_object = other
                        .iter()
                        .find(|x| x.name == object.name)
                        .expect("could not find object to copy from");
                    generate_object(f, other_object, locales)?;
                }
                _ => panic!("only a string value is accepted for key \"copy\""),
            }
            continue;
        }

        if group.len() == 1 && group[0].len() == 0 {
            return Ok(());
        } else if group.len() == 1 && group[0].len() == 1 {
            let singleton = &group[0][0];

            match singleton {
                parser::Value::Raw(x) | parser::Value::String(x) => write!(
                    f,
                    r#"
                    /// `{x:?}`
                    pub const {key}: &'static str = {x:?};
                    "#,
                    key = key,
                    x = x
                )?,
                parser::Value::Integer(x) => write!(
                    f,
                    r#"
                    /// `{x:?}`
                    pub const {key}: i64 = {x:?};
                    "#,
                    key = key,
                    x = x
                )?,
            }
        } else if group.len() == 1 && group[0].iter().map(u8::from).all_equal() {
            let values = &group[0];
            let formatted = values.iter().map(|x| format!("{}", x)).join(", ");

            match &values[0] {
                parser::Value::Raw(_) | parser::Value::String(_) => write!(
                    f,
                    r#"
                    /// `&[{x}]`
                    pub const {key}: &'static [&'static str] = &[{x}];
                    "#,
                    key = key,
                    x = formatted
                )?,
                parser::Value::Integer(_) => write!(
                    f,
                    r#"
                    /// `&[{x}]`
                    pub const {key}: &'static [i64] = &[{x}];
                    "#,
                    key = key,
                    x = formatted
                )?,
            }
        } else if group
            .iter()
            .map(|x| x.iter().map(u8::from))
            .flatten()
            .all_equal()
        {
            write!(
                f,
                r#"
                /// ```ignore
                /// &[
                "#,
            )?;

            for values in group.iter() {
                write!(
                    f,
                    r#"
                    ///     &[{}],
                    "#,
                    values.iter().map(|x| format!("{}", x)).join(", "),
                )?;
            }

            write!(
                f,
                r#"
                /// ]
                /// ```
                "#,
            )?;

            match group[0][0] {
                parser::Value::Raw(_) | parser::Value::String(_) => write!(
                    f,
                    r#"
                    pub const {}: &'static [&'static [&'static str]] = &[
                    "#,
                    key
                )?,
                parser::Value::Integer(_) => write!(
                    f,
                    r#"
                    pub const {}: &'static [&'static [i64]] = &[
                    "#,
                    key,
                )?,
            }

            for values in group.iter() {
                write!(
                    f,
                    r#"
                    &[{}],
                    "#,
                    values.iter().map(|x| format!("{}", x)).join(", "),
                )?;
            }

            write!(
                f,
                r#"
                ];
                "#,
            )?;
        } else {
            unimplemented!("mixed types");
        }
    }

    Ok(())
}

fn generate_locale(
    f: &mut fmt::Formatter,
    lang_normalized: &str,
    objects: &Vec<parser::Object>,
    locales: &HashMap<String, Vec<parser::Object>>,
) -> fmt::Result {
    write!(
        f,
        r#"
        #[allow(non_snake_case,non_camel_case_types,dead_code,unused_imports)]
        pub mod {} {{
        "#,
        lang_normalized,
    )?;

    for object in objects.iter().sorted_by_key(|x| x.name.to_string()) {
        if object.name == "LC_COLLATE"
            || object.name == "LC_CTYPE"
            || object.name == "LC_MEASUREMENT"
            || object.name == "LC_PAPER"
            || object.name == "LC_NAME"
        {
            continue;
        } else if object.values.len() == 1 {
            let (key, value) = &object.values[0];
            match key.as_str() {
                "copy" => {
                    assert_eq!(value.len(), 1);
                    match &value[0] {
                        parser::Value::String(x) => write!(
                            f,
                            r#"
                            pub use super::{}::{};
                            "#,
                            x.replace("@", "_"),
                            object.name,
                        )?,
                        x => panic!("unexpected value for key {}: {:?}", key, x),
                    }
                }
                _ => {}
            }
        } else {
            write!(
                f,
                r#"
                pub mod {} {{
                "#,
                object.name,
            )?;
            generate_object(f, &object, locales)?;
            write!(
                f,
                r#"
                }}

                "#,
            )?;
        }
    }

    write!(
        f,
        r#"
        }}

        "#,
    )
}

fn generate_variants(f: &mut fmt::Formatter, langs: &[(&str, &str)]) -> fmt::Result {
    write!(
        f,
        r#"
        #[allow(non_camel_case_types,dead_code)]
        #[derive(Debug, Copy, Clone, PartialEq)]
        pub enum Locale {{
        "#,
    )?;

    for (lang, norm) in langs {
        write!(
            f,
            r#"
            /// {lang}
            {norm},
            "#,
            lang = lang,
            norm = norm,
        )?;
    }

    write!(
        f,
        r#"
        }}

        impl core::convert::TryFrom<&str> for Locale {{
            type Error = UnknownLocale;

            fn try_from(i: &str) -> Result<Self, Self::Error> {{
                match i {{
        "#,
    )?;

    for (lang, norm) in langs {
        write!(
            f,
            r#"
            {lang:?} => Ok(Locale::{norm}),
            "#,
            lang = lang,
            norm = norm,
        )?;
    }

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

    for (_, norm) in langs {
        write!(
            f,
            r#"
            $crate::Locale::{norm} => $crate::{norm}::$($item)::+,
            "#,
            norm = norm,
        )?;
    }

    write!(
        f,
        r#"
                }}
            }}}}
        }}

        "#,
    )
}

pub struct CodeGenerator(pub HashMap<String, Vec<parser::Object>>);

impl fmt::Display for CodeGenerator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            r#"
            #![no_std]

            #[derive(Debug)]
            pub struct UnknownLocale;

            "#,
        )?;

        let locales = &self.0;

        let normalized: HashMap<_, _> = locales
            .iter()
            .map(|(lang, _)| (lang, lang.replace("@", "_")))
            .collect();

        let mut sorted: Vec<_> = locales.iter().collect();
        sorted.sort_unstable_by_key(|(lang, _)| lang.to_string());
        for (lang, objects) in sorted.iter() {
            generate_locale(f, normalized[lang].as_str(), &objects, locales)?;
        }

        let mut sorted: Vec<_> = locales
            .iter()
            .map(|(lang, _)| (lang.as_str(), normalized[lang].as_str()))
            .collect();
        sorted.sort_unstable_by_key(|(lang, _)| lang.to_string());
        generate_variants(f, &sorted)
    }
}
