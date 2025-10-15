use anyhow::{bail, Result};
use nom::{
    branch::alt,
    bytes::complete::{tag, take, take_while, take_while1},
    character::complete::{
        alpha1, anychar, char, hex_digit1, multispace0, multispace1, not_line_ending, one_of,
        space1,
    },
    combinator::{all_consuming, cut, map, map_opt, map_parser, map_res, opt, verify},
    error::{context, ErrorKind},
    multi::{fold_many0, fold_many1, many0, many1, separated_list0},
    sequence::{preceded, separated_pair, terminated},
    IResult, Parser,
};

#[derive(Debug, PartialEq)]
pub enum Value {
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
            Value::Raw(x) | Value::String(x) => write!(f, "{}", x), // FIXME
            Value::Integer(x) => write!(f, "{:?}", x),
        }
    }
}

fn sp(
    i: &str,
    escape_char: char,
    comment_char: char,
) -> IResult<&str, Vec<&str>, (&str, ErrorKind)> {
    let chars = "\n\r";

    many0(alt((
        space1,
        preceded(
            char(comment_char),
            take_while(move |c| !chars.contains(c) && c != escape_char),
        ),
        preceded(char(escape_char), take(1_usize)),
    )))
    .parse(i)
}

fn integer(i: &str) -> IResult<&str, &str, (&str, ErrorKind)> {
    let chars = "-0123456789";

    take_while1(move |c| chars.contains(c))(i)
}

fn parse_key(i: &str) -> IResult<&str, String, (&str, ErrorKind)> {
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
    ))
    .parse(i)
}

fn parse_raw(
    i: &str,
    escape_char: char,
    comment_char: char,
) -> IResult<&str, String, (&str, ErrorKind)> {
    let chars = " \t\r\n;";

    fold_many1(
        alt((
            take_while1(move |c| !chars.contains(c) && c != comment_char && c != escape_char),
            preceded(char(escape_char), take(1_usize)),
        )),
        String::new,
        |mut acc, item| {
            acc.push_str(item);
            acc
        },
    )
    .parse(i)
}

fn parse_str(i: &str, escape_char: char) -> IResult<&str, String, (&str, ErrorKind)> {
    fold_many0(
        map_parser(
            alt((
                take_while1(|c| c != escape_char && c != '"'),
                map(preceded(char(escape_char), char('\n')), |_| ""),
                preceded(char(escape_char), take(1_usize)),
            )),
            unescape_unicode,
        ),
        String::new,
        |mut acc, item| {
            acc.push_str(item.as_str());
            acc
        },
    )
    .parse(i)
}

fn string(i: &str, escape_char: char) -> IResult<&str, String, (&str, ErrorKind)> {
    context(
        "string",
        alt((
            map(tag("\"\""), |_| String::new()),
            preceded(
                char('\"'),
                cut(terminated(|x| parse_str(x, escape_char), char('\"'))),
            ),
        )),
    )
    .parse(i)
}

fn unescape_unicode(i: &str) -> IResult<&str, String, (&str, ErrorKind)> {
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
    )
    .parse(i)
}

fn parse_special_chars(mut i: &str) -> IResult<&str, (char, char), (&str, ErrorKind)> {
    let mut comment_char = '%';
    let mut escape_char = '/';

    for _ in 0..2 {
        let (rest, (k, c)) = separated_pair(
            preceded(multispace0, alt((tag("comment_char"), tag("escape_char")))),
            space1,
            anychar,
        )
        .parse(i)?;
        i = rest;

        match k {
            "comment_char" => comment_char = c,
            "escape_char" => escape_char = c,
            _ => unreachable!(),
        }
    }

    Ok((i, (comment_char, escape_char)))
}

#[allow(clippy::type_complexity)]
fn key_value(
    i: &str,
    escape_char: char,
    comment_char: char,
) -> IResult<&str, (String, Vec<Option<Value>>), (&str, ErrorKind)> {
    alt((
        separated_pair(
            preceded(|x| sp_comment(x, comment_char), parse_key),
            many1(alt((space1, preceded(char(escape_char), take(1_usize))))),
            separated_list0(one_of("; \t"), opt(|x| value(x, escape_char, comment_char))),
        ),
        map(preceded(|x| sp_comment(x, comment_char), parse_key), |x| {
            (x, Vec::new())
        }),
    ))
    .parse(i)
}

fn value(
    i: &str,
    escape_char: char,
    comment_char: char,
) -> IResult<&str, Value, (&str, ErrorKind)> {
    preceded(
        |x| sp(x, escape_char, comment_char),
        alt((
            map_res(integer, |s| s.parse().map(Value::Integer)),
            map(|x| string(x, escape_char), Value::String),
            map(|x| parse_raw(x, escape_char, comment_char), Value::Raw),
        )),
    )
    .parse(i)
}

#[derive(Debug, PartialEq)]
pub struct Object {
    pub name: String,
    pub values: Vec<(String, Vec<Value>)>,
}

fn parse_object_head(i: &str) -> IResult<&str, &str, (&str, ErrorKind)> {
    let chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZ_";

    take_while1(move |c| chars.contains(c))(i)
}

fn sp_comment(i: &str, comment_char: char) -> IResult<&str, Vec<&str>, (&str, ErrorKind)> {
    many0(alt((
        preceded(char(comment_char), not_line_ending),
        multispace1,
    )))
    .parse(i)
}

fn object(
    i: &str,
    escape_char: char,
    comment_char: char,
) -> IResult<&str, Object, (&str, ErrorKind)> {
    let (i, name) = preceded(|x| sp_comment(x, comment_char), parse_object_head).parse(i)?;
    let (i, values) = preceded(
        multispace0,
        many0(|x| key_value(x, escape_char, comment_char)),
    )
    .parse(i)?;
    let (i, _) = preceded(
        |x| sp_comment(x, comment_char),
        terminated(tag(format!("END {}", name).as_str()), multispace0),
    )
    .parse(i)?;

    Ok((
        i,
        Object {
            name: name.to_string(),
            values: values
                .into_iter()
                .map(|(k, v)| (k, v.into_iter().flatten().collect()))
                .collect(),
        },
    ))
}

fn parse_locale(mut i: &str) -> IResult<&str, Vec<Object>, (&str, ErrorKind)> {
    let mut objects = Vec::new();
    // NOTE: the default comment_char is # because it's used in iso14651_t1_pinyin
    // NOTE: I don't know the default escape_char
    let (rest, special_chars) = opt(parse_special_chars).parse(i)?;
    i = rest;
    let (comment_char, escape_char) = special_chars.unwrap_or(('#', '\0'));

    while !i.is_empty() {
        match object(i, escape_char, comment_char) {
            Ok(x) => {
                let (rest, o) = x;
                i = rest;
                objects.push(o);
            }
            _ => {
                let (rest, _) = all_consuming(|x| sp_comment(x, comment_char)).parse(i)?;
                i = rest;
                if i.is_empty() {
                    break;
                }
            }
        }
    }

    Ok((i, objects))
}

pub fn parse(input: &str) -> Result<Vec<Object>> {
    match parse_locale(input) {
        Ok((_, objects)) => Ok(objects),
        Err(err) => bail!("could not parse input: {}", err),
    }
}

pub fn parse_lang(input: &str) -> Result<(&str, Option<&str>, Option<&str>)> {
    #[allow(clippy::type_complexity)]
    fn inner_parser(
        i: &str,
    ) -> IResult<&str, (&str, Option<&str>, Option<&str>), (&str, ErrorKind)> {
        let (i, lang) = verify(alpha1, |x: &str| x != "translit").parse(i)?;
        let (i, country) = opt(preceded(char('_'), alpha1)).parse(i)?;
        let (i, variant) = all_consuming(opt(preceded(char('@'), alpha1))).parse(i)?;
        Ok((i, (lang, country, variant)))
    }

    match inner_parser(input) {
        Ok((_, objects)) => Ok(objects),
        Err(err) => bail!("could not parse lang: {}", err),
    }
}
