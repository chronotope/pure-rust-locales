#![allow(unreachable_code,dead_code)]

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use itertools::Itertools;

use nom::{
  IResult,
  branch::alt,
  bytes::complete::{tag, take_while1, take},
  combinator::{map_res, cut, map, map_parser, map_opt},
  sequence::{preceded, separated_pair, terminated},
  character::complete::{space1, one_of, char, not_line_ending, multispace1, multispace0, hex_digit1},
  error::{context, ParseError, ErrorKind},
  multi::{many0,separated_list,fold_many0},
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

/*
impl Value {
    fn from_str<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, Value, E> {
        map(many0(alt((
            none_of("/"),
            preceded(char('/'), anychar),
        ))), |sub| Value::String(sub.into_iter().collect()))(i)
    }
}
*/

struct KeyValuePair {
    key: String,
    value: Value,
}

impl Value {
    fn as_char(&self) -> Option<char> {
        match self {
            Value::Raw(s) => s.chars().exactly_one().ok(),
            _ => None,
        }
    }
}

fn sp<'a, E: ParseError<&'a str>>(i: &'a str, escape_char: char) -> IResult<&'a str, Vec<&'a str>, E> {
    many0(
        alt((
            multispace1,
            preceded(char(escape_char), take(1_usize)),
            //preceded(char(comment_char), not_line_ending),
        ))
    )(i)
}

fn integer<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, &'a str, E> {
  let chars = "-0123456789";

  take_while1(move |c| chars.contains(c))(i)
}

fn parse_key<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, String, E> {
    let chars = "abcdefghijklmnopqrstuvwxyz0123456789_-";

    alt((
        map(take_while1(move |c| chars.contains(c)), |x: &str| x.to_string()),
        //take_while1(move |c| chars.contains(c)),
        /*
        map_res(
            preceded(tag("<U"), terminated(hex_digit1, char('>'))),
            |x: &str| u32::from_str_radix(x, 16).map(|x| std::char::from_u32(x).unwrap_or('?').to_string()),
        ),
        */
        map(
            preceded(char('<'), terminated(take_while1(|c| c != '>'), char('>'))),
            |x: &str| x.to_string(),
        ),
        map(tag(".."), |x: &str| x.to_string()),
    ))(i)
}

fn parse_raw<'a, E: ParseError<&'a str>>(i: &'a str, comment_char: char) -> IResult<&'a str, &'a str, E> {
  let chars = " \t\r\n;";

  take_while1(move |c| !chars.contains(c) && c != comment_char)(i)
}

/*
fn parse_str<'a, E: ParseError<&'a str>>(i: &'a str, escape_char: char) -> IResult<&'a str, String, E> {
    let (i, s) = escaped_transform(take_while1(|c| c != '"' && c != escape_char), escape_char, |i| map(anychar, |x| x)(i))(i)?;
    let (_, x) = unescape_unicode::<(&str, ErrorKind)>(s.as_str()).expect("could not decode unicode");

    Ok((i, x))
}
*/

fn parse_str<'a, E: ParseError<&'a str>>(i: &'a str, escape_char: char) -> IResult<&'a str, String, E> {
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
    )
    (i)
}

fn string<'a, E: ParseError<&'a str>>(i: &'a str, escape_char: char) -> IResult<&'a str, String, E> {
  context("string",
      alt((
      map(tag("\"\""), |_| String::new()),
    preceded(
      char('\"'),
      cut(terminated(
          |x| parse_str(x, escape_char),
          char('\"')
      ))
  ))))(i)
}

fn unescape_unicode<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, String, E> {
    map(
    many0(
    alt((
        map(take_while1(|c| c != '<'), |x: &str| x.to_string()),
        map_opt(
            map_res(
                preceded(tag("<U"), terminated(hex_digit1, char('>'))),
                |x: &str| u32::from_str_radix(x, 16),
            ),
            |x: u32| std::char::from_u32(x).map(|x| x.to_string()),
        ),
    ))
    )
    , |x: Vec<String>| x.join(""))
    (i)
}

fn key_value<'a, E: ParseError<&'a str>>(i: &'a str, escape_char: char, comment_char: char) -> IResult<&'a str, (String, Vec<Value>), E> {
    alt((
        //separated_pair(preceded(sp_comment, parse_key), space1, separated_list(char(';'), |x| value(x, escape_char, comment_char))),
        separated_pair(preceded(|x| sp_comment(x, comment_char), parse_key), space1, separated_list(one_of("; "), |x| value(x, escape_char, comment_char))),
        //separated_pair(preceded(sp_comment, parse_key), space1, separated_list(space1, |x| value(x, escape_char, comment_char))),
        //separated_pair(preceded(sp_comment, parse_key), space1, many1(|x| value(x, escape_char, comment_char))),
        map(preceded(|x| sp_comment(x, comment_char), parse_key), |x| (x, Vec::new())),
    ))(i)
}

/// here, we apply the space parser before trying to parse a value
fn value<'a, E: ParseError<&'a str>>(i: &'a str, escape_char: char, comment_char: char) -> IResult<&'a str, Value, E> {
  preceded(
    |x| sp(x, escape_char),
    alt((
      map_res(integer, |s| i64::from_str_radix(s, 10).map(Value::Integer)),
      map(|x| string(x, escape_char), Value::String),
      map(|x| parse_raw(x, comment_char), |s| Value::Raw(String::from(s))),
    )),
  )(i)
}

#[derive(Debug, PartialEq)]
struct Object {
    name: String,
    values: Vec<(String, Vec<Value>)>,
}

impl Object {
    fn memory_size(&self) -> usize {
        let mut values_capacity = 0;
        
        for (key, values) in self.values.iter() {
            values_capacity += key.capacity();
            values_capacity += values.capacity() * std::mem::size_of::<Value>();
        }

        self.name.capacity() + values_capacity
    }
}

fn parse_object_head<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, &'a str, E> {
  let chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZ_";

  take_while1(move |c| chars.contains(c))(i)
}

fn sp_comment<'a, E: ParseError<&'a str>>(i: &'a str, comment_char: char) -> IResult<&'a str, Vec<&'a str>, E> {
many0(
    alt((
        preceded(char(comment_char), not_line_ending),
        multispace1,
    ))
)
(i)
}

fn object<'a, E: ParseError<&'a str>>(i: &'a str, escape_char: char, comment_char: char) -> IResult<&'a str, Object, E> {
    let (i, name) = preceded(|x| sp_comment(x, comment_char), parse_object_head)(i)?;
    let (i, values) = preceded(multispace0, many0(|x| key_value(x, escape_char, comment_char)))(i)?;
    //eprintln!("{:?}", values);
    let (i, _) = preceded(|x| sp_comment(x, comment_char), terminated(tag(format!("END {}", name).as_str()), multispace0))(i)?;

    Ok((i, Object {
        name: name.to_string(),
        values: values.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
    }))
}

fn read_locale(lang: &str) -> String {
    let input = std::fs::read_to_string(format!("localedata/locales/{}", lang)).unwrap();
    let mut i = input.as_str();
    let mut comment_char = '%';
    let mut escape_char = '/';
    let mut objects = Vec::new();

    for _ in 0..2 {
        let (rest, (k, v)) = key_value::<(&str, ErrorKind)>(i, '\0', '\0').expect("could not parse");
        i = rest;
        match k.as_str() {
            "comment_char" => comment_char = v[0].as_char().expect("invalid comment_char"),
            "escape_char" => escape_char = v[0].as_char().expect("invalid escape_char"),
            _ => panic!(),
        }
    }

    while !i.is_empty() {
        match object::<(&str, ErrorKind)>(i, escape_char, comment_char) {
            Ok(x) => {
                let (rest, o) = x;
                i = rest;
                objects.push(o);
                //eprintln!("result: {:?}", o);
            },
            Err(err) => {
                let (i, _) = sp_comment::<(&str, ErrorKind)>(i, comment_char).unwrap();
                if i.is_empty() {
                    break;
                }
                let mut begin = format!("{:?}", err);
                let mut end = format!("{:?}", err);
                begin.truncate(80);
                end.drain(..(end.len().saturating_sub(80)));
                panic!("{}...{} {}", begin, end, i);
            }
        }
    }
    //panic!("{}", objects.iter().map(|x| x.memory_size()).fold(0, |acc, x| acc + x));

    let mut result = String::new();

    result.push_str("#[allow(non_snake_case,non_camel_case_types,dead_code,unused_imports)]\n\n");
    result.push_str(&format!("pub mod {} {{\n", lang));
    result.push_str("    use crate::types::Value;\n\n");

    for o in objects.iter() {
        result.push_str(&format!("    pub struct {};\n\n", o.name));
        result.push_str(&format!("    impl {} {{\n", o.name));
        for (key, group) in &o.values.iter().group_by(|x| x.0.clone()) {
            let group: Vec<_> = group.map(|x| &x.1).collect();

            if group.len() == 1 && group[0].len() == 1 {
                let singleton = &group[0][0];

                result.push_str(&match singleton {
                    Value::Raw(x) | Value::String(x) => format!("        pub fn {}() -> &'static str {{ {:?} }}\n", key, x),
                    Value::Integer(x) => format!("        pub fn {}() -> i64 {{ {:?} }}\n", key, x),
                });
            } else if group.len() == 1 && group[0].iter().map(u8::from).all_equal() {
                let values = &group[0];
                let formatted = values.iter().map(|x| format!("{}", x)).join(", ");

                result.push_str(&match values[0] {
                    Value::Raw(_) | Value::String(_) => format!("        pub fn {}() -> &'static [&'static str] {{ &[{}] }}\n", key, formatted),
                    Value::Integer(_) => format!("        pub fn {}() -> &'static [i64] {{ &[{}] }}\n", key, formatted),
                });
            } else if group.iter().map(|x| x.iter().map(u8::from)).flatten().all_equal() {
                result.push_str(&match group[0][0] {
                    Value::Raw(_) | Value::String(_) => format!("        pub fn {}() -> &'static [&'static [&'static str]] {{ &[", key),
                    Value::Integer(_) => format!("        pub fn {}() -> &'static [&'static [i64]] {{ &[", key),
                });
                for values in group {
                    result.push_str("&[");
                    result.push_str(&values.iter().map(|x| format!("{}", x)).join(", "));
                    result.push_str("], ");
                }
                result.push_str("] }\n");
            } else {
                result.push_str(&format!("        pub fn {}() -> &'static[&'static [&'static Value]] {{ &[", key));
                for values in group {
                    result.push_str("&[");
                    result.push_str(&values.iter().map(|x| match x {
                            Value::String(x) => format!("&Value::String({:?})", x),
                            Value::Raw(x) => format!("&Value::String({:?})", x),
                            Value::Integer(x) => format!("&Value::Integer({})", x),
                        }).join(", "));
                    result.push_str("], ");
                }
                result.push_str("] }\n");
            }
        }
        result.push_str("    }\n\n");
    }

    result.push_str("}\n");
    //panic!("{}", result);

    result
}

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("hello.rs");
    let mut f = File::create(&dest_path).unwrap();

    f.write_all(read_locale("fr_BE").as_bytes()).unwrap();
}
