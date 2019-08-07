use quick_xml::events::Event;
use std::io::BufRead;

use crate::error::Error;

#[macro_export]
macro_rules! try_some {
    ($e:expr) => (
        match $e {
            Ok(x) => x,
            Err(err) => return Some(Err(err)),
        }
    )
}

//parse_struct_fields_update
#[macro_export]
macro_rules! parse_struct_update {
    ($rdr:expr,
     $buf:expr,
     $xml_element:expr,
     $data_struct:ident,
     {$($xml_field:expr => $data_struct_field:ident),* $(,)?},
     {$($xml_field_opt:expr => $data_struct_field_opt:ident),* $(,)?}
     ) => (
        match $rdr.read_event($buf) {
            Ok(Event::Start(ref e)) => {
                match e.name() {
                    b"document-id" => {
                        loop {
                            match $rdr.read_event($buf) {
                                Ok(Event::Start(ref e)) => {
                                    match e.name() {
                                        $($xml_field => $data_struct.$data_struct_field = deser_text_from(e.name(), $rdr,)?,)+
                                        $($xml_field_opt => $data_struct.$data_struct_field_opt = Some(deser_text_from(e.name(), $rdr,)?),)*
                                        _ => return Err(Error::Deser { src: format!("unrecognized element {:?} in {}", std::str::from_utf8(e.name()), $xml_element) }),
                                    }
                                },
                                Ok(Event::End(ref e)) => {
                                    if e.name() == $xml_element.as_bytes() { break };
                                },
                                _ => break,
                            }
                        }
                    }
                    _ => return Err(Error::Deser { src: format!("found element {:?}, not {}", std::str::from_utf8(e.name()), $xml_element) }),
                }
            },
            Ok(_) => return Err(Error::Deser { src: format!("found non-start-element besides {}", $xml_element) }),

            Err(err) => return Err(Error::Deser { src: err.to_string() }),
        }
    )
}

//parse_struct_fields_update
//This one doesn't expect an open tag (called after open tag is already encountered)
#[macro_export]
macro_rules! parse_struct_update_from {
    ($rdr:expr,
     $buf:expr,
     $xml_element:expr,
     $data_struct:ident,
     {$($xml_field:expr => $data_struct_field:ident),* $(,)?},
     {$($xml_field_opt:expr => $data_struct_field_opt:ident),* $(,)?}
     ) => (
        loop {
            match $rdr.read_event($buf) {
                Ok(Event::Start(ref e)) => {
                    match e.name() {
                        $($xml_field => $data_struct.$data_struct_field = deser_text_from(e.name(), $rdr,)?,)+
                        $($xml_field_opt => $data_struct.$data_struct_field_opt = Some(deser_text_from(e.name(), $rdr,)?),)*
                        _ => return Err(Error::Deser { src: format!("unrecognized element {:?} in {}", std::str::from_utf8(e.name()), $xml_element) }),
                    }
                },
                Ok(Event::End(ref e)) => {
                    if e.name() == $xml_element.as_bytes() { break };
                },
                _ => break,
            }
        }
    )
}

// consumes a start tag, to just advance one deeper in nesting
pub fn consume_start<B: BufRead>(
    rdr: &mut quick_xml::Reader<B>,
    buf: &mut Vec<u8>,
    xml_element: &[u8],
    ) -> Result<(), Error>
{
    match rdr.read_event(buf) {
        Ok(Event::Start(ref e)) => {
            if e.name() == xml_element {
                Ok(())
            } else {
                Err(Error::Deser { src: format!("found element {:?}, not {:?}", std::str::from_utf8(e.name()), std::str::from_utf8(xml_element)) })
            }
        },
        Ok(_) => Err(Error::Deser { src: format!("found non-start-element besides {:?}", std::str::from_utf8(xml_element)) }),
        Err(err) => Err(Error::Deser { src: err.to_string() }),
    }
}
