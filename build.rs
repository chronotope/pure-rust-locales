use itertools::Itertools;
use std::collections::HashMap;
use std::env;
use std::fs::{read_dir, File};
use std::io::Write;
use std::path::Path;

const LOCALES: &str = "localedata/locales";

use nom::{
    branch::alt,
    bytes::complete::{tag, take, take_while, take_while1},
    character::complete::{
        alpha1, anychar, char, hex_digit1, multispace0, multispace1, not_line_ending, one_of,
        space1,
    },
    combinator::{all_consuming, cut, map, map_opt, map_parser, map_res, opt, verify},
    error::{context, ErrorKind, ParseError},
    multi::{fold_many0, fold_many1, many0, many1, separated_list},
    sequence::{preceded, separated_pair, terminated},
    IResult,
};

#[derive(Debug, PartialEq)]
enum Value {
    Raw(String),
    String(String),
    Integer(i64),
}

impl From<&Value> for u8 {
    fn from(x: &Value) -> u8 {
        match x {
            Value::Raw(_) | Value::String(_) => 0,
            Value::Integer(_) => 1,
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Raw(x) | Value::String(x) => write!(f, "{:?}", x),
            Value::Integer(x) => write!(f, "{:?}", x),
        }
    }
}

fn sp<'a, E: ParseError<&'a str>>(
    i: &'a str,
    escape_char: char,
    comment_char: char,
) -> IResult<&'a str, Vec<&'a str>, E> {
    let chars = "\n\r";

    many0(alt((
        space1,
        preceded(
            char(comment_char),
            take_while(move |c| !chars.contains(c) && c != escape_char),
        ),
        preceded(char(escape_char), take(1_usize)),
    )))(i)
}

fn integer<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, &'a str, E> {
    let chars = "-0123456789";

    take_while1(move |c| chars.contains(c))(i)
}

fn parse_key<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, String, E> {
    let chars = "abcdefghijklmnopqrstuvwxyz0123456789_-";

    alt((
        map(take_while1(move |c| chars.contains(c)), |x: &str| {
            x.to_string()
        }),
        map(
            preceded(char('<'), terminated(take_while1(|c| c != '>'), char('>'))),
            |x: &str| x.to_string(),
        ),
        map(alt((tag(".."), tag("UNDEFINED"))), |x: &str| x.to_string()),
    ))(i)
}

fn parse_raw<'a, E: ParseError<&'a str>>(
    i: &'a str,
    escape_char: char,
    comment_char: char,
) -> IResult<&'a str, String, E> {
    let chars = " \t\r\n;";

    fold_many1(
        alt((
            take_while1(move |c| !chars.contains(c) && c != comment_char && c != escape_char),
            preceded(char(escape_char), take(1_usize)),
        )),
        String::new(),
        |mut acc, item| {
            acc.push_str(item);
            acc
        },
    )(i)
}

fn parse_str<'a, E: ParseError<&'a str>>(
    i: &'a str,
    escape_char: char,
) -> IResult<&'a str, String, E> {
    fold_many0(
        map_parser(
            alt((
                take_while1(|c| c != escape_char && c != '"'),
                preceded(char(escape_char), take(1_usize)),
            )),
            unescape_unicode,
        ),
        String::new(),
        |mut acc, item| {
            acc.push_str(item.as_str());
            acc
        },
    )(i)
}

fn string<'a, E: ParseError<&'a str>>(
    i: &'a str,
    escape_char: char,
) -> IResult<&'a str, String, E> {
    context(
        "string",
        alt((
            map(tag("\"\""), |_| String::new()),
            preceded(
                char('\"'),
                cut(terminated(|x| parse_str(x, escape_char), char('\"'))),
            ),
        )),
    )(i)
}

fn unescape_unicode<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, String, E> {
    map(
        many0(alt((
            map(take_while1(|c| c != '<'), |x: &str| x.to_string()),
            map_opt(
                map_res(
                    preceded(tag("<U"), terminated(hex_digit1, char('>'))),
                    |x: &str| u32::from_str_radix(x, 16),
                ),
                |x: u32| std::char::from_u32(x).map(|x| x.to_string()),
            ),
        ))),
        |x: Vec<String>| x.join(""),
    )(i)
}

fn parse_special_chars<'a, E: ParseError<&'a str>>(
    mut i: &'a str,
) -> IResult<&'a str, (char, char), E> {
    let mut comment_char = '%';
    let mut escape_char = '/';

    for _ in 0..2 {
        let (rest, (k, c)) = separated_pair(
            preceded(multispace0, alt((tag("comment_char"), tag("escape_char")))),
            space1,
            anychar,
        )(i)?;
        i = rest;

        match k {
            "comment_char" => comment_char = c,
            "escape_char" => escape_char = c,
            _ => unreachable!(),
        }
    }

    Ok((i, (comment_char, escape_char)))
}

fn key_value<'a, E: ParseError<&'a str>>(
    i: &'a str,
    escape_char: char,
    comment_char: char,
) -> IResult<&'a str, (String, Vec<Option<Value>>), E> {
    alt((
        separated_pair(
            preceded(|x| sp_comment(x, comment_char), parse_key),
            many1(alt((space1, preceded(char(escape_char), take(1_usize))))),
            separated_list(one_of("; \t"), opt(|x| value(x, escape_char, comment_char))),
        ),
        map(preceded(|x| sp_comment(x, comment_char), parse_key), |x| {
            (x, Vec::new())
        }),
    ))(i)
}

fn value<'a, E: ParseError<&'a str>>(
    i: &'a str,
    escape_char: char,
    comment_char: char,
) -> IResult<&'a str, Value, E> {
    preceded(
        |x| sp(x, escape_char, comment_char),
        alt((
            map_res(integer, |s| i64::from_str_radix(s, 10).map(Value::Integer)),
            map(|x| string(x, escape_char), Value::String),
            map(
                |x| parse_raw(x, escape_char, comment_char),
                |s| Value::Raw(String::from(s)),
            ),
        )),
    )(i)
}

#[derive(Debug, PartialEq)]
struct Object {
    name: String,
    values: Vec<(String, Vec<Value>)>,
}

fn parse_object_head<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, &'a str, E> {
    let chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZ_";

    take_while1(move |c| chars.contains(c))(i)
}

fn sp_comment<'a, E: ParseError<&'a str>>(
    i: &'a str,
    comment_char: char,
) -> IResult<&'a str, Vec<&'a str>, E> {
    many0(alt((
        preceded(char(comment_char), not_line_ending),
        multispace1,
    )))(i)
}

fn object<'a, E: ParseError<&'a str>>(
    i: &'a str,
    escape_char: char,
    comment_char: char,
) -> IResult<&'a str, Object, E> {
    let (i, name) = preceded(|x| sp_comment(x, comment_char), parse_object_head)(i)?;
    let (i, values) = preceded(
        multispace0,
        many0(|x| key_value(x, escape_char, comment_char)),
    )(i)?;
    let (i, _) = preceded(
        |x| sp_comment(x, comment_char),
        terminated(tag(format!("END {}", name).as_str()), multispace0),
    )(i)?;

    Ok((
        i,
        Object {
            name: name.to_string(),
            values: values
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.into_iter().filter_map(|x| x).collect()))
                .collect(),
        },
    ))
}

fn parse_locale<'a, E: ParseError<&'a str>>(mut i: &'a str) -> IResult<&'a str, Vec<Object>, E> {
    let mut objects = Vec::new();
    // NOTE: the default comment_char is # because it's used in iso14651_t1_pinyin
    // NOTE: I don't know the default escape_char
    let (rest, special_chars) = opt(parse_special_chars)(i)?;
    i = rest;
    let (comment_char, escape_char) = special_chars.unwrap_or(('#', '\0'));

    while !i.is_empty() {
        match object::<(&str, ErrorKind)>(i, escape_char, comment_char) {
            Ok(x) => {
                let (rest, o) = x;
                i = rest;
                objects.push(o);
            }
            _ => {
                let (rest, _) = all_consuming(|x| sp_comment(x, comment_char))(i)?;
                i = rest;
                if i.is_empty() {
                    break;
                }
            }
        }
    }

    Ok((i, objects))
}

fn generate_object(object: &Object, locales: &HashMap<String, Vec<Object>>) -> String {
    let mut result = String::new();

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
                Value::String(other_lang) => {
                    let other = locales
                        .get(other_lang)
                        .expect(&format!("unknown locale: {}", other_lang));
                    let other_object = other
                        .iter()
                        .find(|x| x.name == object.name)
                        .expect("could not find object to copy from");
                    result.push_str(generate_object(other_object, locales).as_str());
                }
                _ => panic!("only a string value is accepted for key \"copy\""),
            }
            continue;
        }

        if group.len() == 1 && group[0].len() == 0 {
            return result;
        } else if group.len() == 1 && group[0].len() == 1 {
            let singleton = &group[0][0];

            result.push_str(&match singleton {
                Value::Raw(x) | Value::String(x) => format!(
                    "        /// `{x:?}`\n        pub const {}: &'static str = {x:?};\n",
                    key,
                    x = x
                ),
                Value::Integer(x) => format!(
                    "        /// `{x:?}`\n        pub const {}: i64 = {x:?};\n",
                    key,
                    x = x
                ),
            });
        } else if group.len() == 1 && group[0].iter().map(u8::from).all_equal() {
            let values = &group[0];
            let formatted = values.iter().map(|x| format!("{}", x)).join(", ");

            result.push_str(&match values[0] {
                Value::Raw(_) | Value::String(_) => format!(
                    "        /// `&[{x}]`\n        pub const {}: &'static [&'static str] = &[{x}];\n",
                    key,
                    x = formatted
                ),
                Value::Integer(_) => format!(
                    "        /// `&[{x}]`\n        pub const {}: &'static [i64] = &[{}];\n",
                    key,
                    x = formatted
                ),
            });
        } else if group
            .iter()
            .map(|x| x.iter().map(u8::from))
            .flatten()
            .all_equal()
        {
            result.push_str("        /// ```ignore\n");
            result.push_str("        /// &[\n");
            for values in group.iter() {
                result.push_str("        ///     &[");
                result.push_str(&values.iter().map(|x| format!("{}", x)).join(", "));
                result.push_str("],\n");
            }
            result.push_str("        /// ]\n");
            result.push_str("        /// ```\n");

            result.push_str(&match group[0][0] {
                Value::Raw(_) | Value::String(_) => format!(
                    "        pub const {}: &'static [&'static [&'static str]] = &[",
                    key
                ),
                Value::Integer(_) => {
                    format!("        pub const {}: &'static [&'static [i64]] = &[", key)
                }
            });
            for values in group.iter() {
                result.push_str("&[");
                result.push_str(&values.iter().map(|x| format!("{}", x)).join(", "));
                result.push_str("], ");
            }
            result.push_str("];\n");
        } else {
            unimplemented!("mixed types");
        }
    }

    result
}

fn generate_locale(
    lang: &str,
    objects: &Vec<Object>,
    locales: &HashMap<String, Vec<Object>>,
) -> String {
    let mut result = String::new();

    result.push_str("#[allow(non_snake_case,non_camel_case_types,dead_code,unused_imports)]\n");
    result.push_str(&format!("pub mod {} {{\n", lang.replace("@", "_")));

    for object in objects.iter() {
        if object.name != "LC_COLLATE" && object.name != "LC_CTYPE" {
            result.push_str(&format!("    pub mod {} {{\n", object.name));
            result.push_str(generate_object(&object, locales).as_str());
            result.push_str("    }\n\n");
        }
    }

    result.push_str("}\n\n");

    result
}

fn recognize_lang<'a, E: ParseError<&'a str>>(
    i: &'a str,
) -> IResult<&'a str, (&str, Option<&str>, Option<&str>), E> {
    let (i, lang) = verify(alpha1, |x: &str| x != "translit")(i)?;
    let (i, country) = opt(preceded(char('_'), alpha1))(i)?;
    let (i, variant) = all_consuming(opt(preceded(char('@'), alpha1)))(i)?;

    Ok((i, (lang, country, variant)))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut locales = HashMap::new();

    for entry in read_dir(LOCALES)? {
        let entry = entry?;
        let lang = entry.file_name().to_str().unwrap().to_string();
        let path = entry.path();
        if let Ok(input) = std::fs::read_to_string(path) {
            let (_, objects) =
                parse_locale::<(&str, ErrorKind)>(input.as_str()).expect("could not parse");
            locales.insert(lang.to_string(), objects);
        }
    }

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("locales.rs");
    let mut f = File::create(&dest_path)?;

    for (lang, objects) in locales.iter() {
        if let Ok(_) = recognize_lang::<(&str, ErrorKind)>(lang.as_str()) {
            let code = generate_locale(lang.as_ref(), &objects, &locales);
            f.write_all(code.as_bytes())?;
        }
    }

    Ok(())
}
