#![no_std]

#[derive(Debug)]
pub struct UnknownLocale;

include!(concat!(env!("OUT_DIR"), "/locales.rs"));
