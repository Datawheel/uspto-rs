use quick_xml::{self, Reader};
use quick_xml::events::{Event, BytesText};
use snafu::OptionExt;
use std::io::BufRead;

use crate::data::*;
use crate::error::Error;
use crate::error::Deser;
// helper macros
use crate::{try_some, parse_struct_update, parse_struct_update_from};

pub struct PatentGrants<B: BufRead> {
    rdr: quick_xml::Reader<B>,
    buf: Vec<u8>,
}

impl<B: BufRead> PatentGrants<B> {
    pub fn from_reader(b: B) -> Self {
        let mut rdr = Reader::from_reader(b);

        // TODO check other options
        rdr.trim_text(true);

        PatentGrants {
            rdr,
            buf: Vec::new(),
        }
    }

    /// main entry point for deserialization
    ///
    /// returns None if no more data
    /// else if there's an error in deser (e.g. partial data)
    /// return Some(Result<_>)
    fn deser_patent_grant(&mut self) -> Option<Result<PatentGrant, Error>> {
        // first skip through headers
        let hdr = deser_header(&mut self.rdr, &mut self.buf);
        match hdr {
            Some(hdr_res) => {
                if let Err(err) = hdr_res {
                    return Some(Err(err));
                }
            },
            None => return None,
        }
        self.buf.clear();

        // if headers are in the right place, we can continue
        let mut patent_grant = PatentGrant::default();

        // deser for each element, update default patent grant
        loop {
            match self.rdr.read_event(&mut self.buf) {
                Ok(Event::PI(pi_bytes)) => {
                    try_some!(deser_top_pi(pi_bytes, &mut self.rdr, &mut patent_grant));
                },
                Ok(Event::Start(ref e)) => {
                    match e.name() {
                        b"us-claim-statement" => {
                            patent_grant.us_claim_statement = try_some!(deser_text(e.name(), &mut self.rdr));
                        },
                        b"claims" => {
                            try_some!(deser_claims(&mut self.rdr, &mut self.buf, &mut patent_grant));
                        },
                        b"us-bibliographic-data-grant" => {
                            try_some!(deser_biblio(&mut self.rdr, &mut self.buf, &mut patent_grant.us_bibliographic_data_grant));
                        },
                        _ => continue,
                    }
                },
                Ok(Event::Eof) => break,
                Ok(Event::End(e)) => {
                    if e.name() == b"us-patent-grant" {
                        break;
                    } else {
                        continue;
                    }
                },
                Ok(_) => continue,
                Err(err) => return Some(Err(Error::Deser { src: err.to_string() })),
            };
        }

        self.buf.clear();

        Some(Ok(patent_grant))
    }
}

impl<B: BufRead> Iterator for PatentGrants<B> {
    type Item = Result<PatentGrant, Error>;

    // clear buf after each PatentGrant;
    // in the future, when GAT lands,
    // the iterator will be able to borrow
    // the underlying data.
    fn next(&mut self) -> Option<Self::Item> {
        let res = self.deser_patent_grant();
        self.buf.clear();

        res
    }
}

// helper fns for deser
// never clear buffer inside fn!

/// only returns None if there's no input. Otherwise
/// tries to parse, and will error if necessary.
fn deser_header<B: BufRead>(rdr: &mut quick_xml::Reader<B>, buf: &mut Vec<u8>) -> Option<Result<(), Error>> {
    // first match xml declaration
    match rdr.read_event(buf) {
        Ok(Event::Decl(_)) => (),
        Ok(Event::Eof) => return None,
        Ok(_) => return Some(Err(Error::Deser { src: "xml decl not found at head of patent grant xml".to_owned() })),
        Err(err) => return Some(Err(Error::Deser { src: err.to_string() })),
    }

    // then match doctype declaration
    match rdr.read_event(buf) {
        Ok(Event::DocType(_)) => Some(Ok(())),
        Ok(Event::Eof) => None,
        Ok(_) => Some(Err(Error::Deser { src: "doctype decl not found at head of patent grant xml".to_owned() })),
        Err(err) => Some(Err(Error::Deser { src: err.to_string() })),
    }
}

/// top level program instruction handling.
/// encompasses all possible descriptions in grant:
/// - brief-description-of-drawings
/// - BRFSUM (brief summary)
/// - RELAPP (other patent relations)
/// - DETDESC (detailed description)
/// - in-line-formulae
///
/// This one is a little more involved. The idea is to go from the top-level program instruction,
/// and find the next top-level instruction that has end = tail. In the meantime, all of the
/// bytes are being written to a new buffer instead of the overall buffer. That means that the
/// new buffer cvan then be converted directly to a string.
///
/// One downside of this string conversion: tags are lost (i guess quick-xml didn't think it needed
/// to save them)
fn deser_top_pi<B: BufRead>(
    pi_bytes: BytesText,
    rdr: &mut quick_xml::Reader<B>,
    patent_grant: &mut PatentGrant
    ) -> Result<(), Error>
{
    let pi_name_res = pi_bytes.unescape_and_decode(&rdr);
    let pi_name = match pi_name_res {
        Ok(ref s) => s.split_whitespace().nth(0).context(Deser { src: "No name for PI".to_string() })?,
        Err(_) => return Err(Error::Deser { src: "No name for PI".into() }),
    };

    let end = match pi_name_res {
        Ok(ref s) => s.split_whitespace().last().context(Deser { src: "No end for PI".to_string() })?,
        Err(_) => return Err(Error::Deser { src: "No end for PI".into() }),
    };

    if end != "end=\"lead\"" {
        // just skip if not lead; it means it's some other top level PI
        return Ok(());
    }

    // get end byte of PI.
    // find beginning byte of next PI.
    // get string in between
    let mut text_buf = Vec::new();
    loop {
        match rdr.read_event(&mut text_buf) {
            Ok(Event::PI(pi_bytes_2)) => {
                // just search for the next tail, don't need to match on name.
                let pi_2_res = pi_bytes_2.unescape_and_decode(&rdr);

                let end = match pi_2_res {
                    Ok(ref s) => s.split_whitespace().last().context(Deser { src: "No end for PI".to_string() })?,
                    Err(_) => return Err(Error::Deser { src: "No end for PI".into() }),
                };

                if end != "end=\"tail\"" {
                    // in case of nested PI; I don't care about them unless they're
                    // one of the description ones, so just grab it as part of text
                    continue;
                }

                break;
            },
            Ok(_) => continue,
            Err(err) => return Err(Error::Deser { src: err.to_string() }),

        }
    }
    let text = match String::from_utf8(text_buf.to_vec()) {
        Ok(s) => s,
        Err(err) => return Err(Error::Deser { src: err.to_string() }),
    };
    patent_grant.descriptions.insert(pi_name.to_string(), text);

    Ok(())
}

fn deser_claims<B: BufRead>(
    rdr: &mut quick_xml::Reader<B>,
    buf: &mut Vec<u8>,
    patent_grant: &mut PatentGrant
    ) -> Result<(), Error>
{
    loop {
        match rdr.read_event(buf) {
            Ok(Event::Start(ref e)) => {
                if e.name() == b"claim" {
                    match rdr.read_event(buf) {
                        Ok(Event::Start(ref e)) => {
                            if e.name() == b"claim-text" {
                                patent_grant.claims.push(deser_text(e.name(), rdr)?);
                            } else {
                                break;
                            }
                        },
                        Ok(_) => break,
                        Err(err) => return Err(Error::Deser { src: err.to_string() }),
                    }
                } else {
                    break; // if no claims, exit
                }
            },
            Ok(_) => break, // if there's no more claims, exit
            Err(err) => return Err(Error::Deser { src: err.to_string() }),
        }
    }

    Ok(())
}

/// call after you hit biblio tag
fn deser_biblio<B: BufRead>(
    rdr: &mut quick_xml::Reader<B>,
    buf: &mut Vec<u8>,
    biblio: &mut BibliographicDataGrant,
    ) -> Result<(), Error>
{
    loop {
        match rdr.read_event(buf) {
            Ok(Event::Start(ref e)) => {
                match e.name() {
                    b"publication-reference" => {
                        deser_doc_id(rdr, buf, &mut biblio.publication_reference)?;
                    },
                    b"application-reference" => {
                        deser_doc_id(rdr, buf, &mut biblio.application_reference)?;
                    },
                    b"us-application-series-code" => {
                        biblio.us_application_series_code = deser_text(e.name(), rdr)?;
                    },
                    b"classification-locarno" => {
                        deser_class_locarno(rdr, buf, &mut biblio.classification_locarno)?;
                    },
                    b"classification-national" => {
                        deser_class_national(rdr, buf, &mut biblio.classification_national)?;
                    },
                    // TODO when all elements in, use this line instead
                    //_ => break,
                    _ => continue,
                }
            },
            Ok(Event::End(ref e)) => {
                if e.name() == b"us-bibliographic-data-grant" {
                    break;
                }
            },
            // TODO when all elements in, use this line instead
            // Ok(_) => return Err(Error::Deser { src: "found non-start-element not in biblio".to_string() }),
            // for now, can just break out of biblio loop
            Ok(_) => continue,
            Err(err) => return Err(Error::Deser { src: err.to_string() }),
        };
    }

    Ok(())
}

/// pub struct DocumentId {
///     pub country: String,
///     pub doc_number: String,
///     pub kind: Option<String>,
///     pub date: String,
/// }
fn deser_doc_id<B: BufRead>(rdr: &mut quick_xml::Reader<B>, buf: &mut Vec<u8>, doc_id: &mut DocumentId) -> Result<(), Error> {
    parse_struct_update!(
        rdr,
        buf,
        "document-id",
        doc_id,
        // Required
        {
            b"country" => country,
            b"doc-number" => doc_number,
            b"date" => date,
        },
        // Option
        {
            b"kind" => kind,
        }
    );

    Ok(())
}

/// pub struct ClassificationLocarno {
///     pub edition: String,
///     pub main_classification: String,
/// }
fn deser_class_locarno<B: BufRead>(
    rdr: &mut quick_xml::Reader<B>,
    buf: &mut Vec<u8>,
    class_locarno: &mut ClassificationLocarno
    ) -> Result<(), Error>
{
    parse_struct_update_from!(
        rdr,
        buf,
        "classification-locarno",
        class_locarno,
        // Required
        {
            b"edition" => edition,
            b"main-classification" => main_classification,
        },
        // Optional
        {}
    );

    Ok(())
}

/// pub struct ClassificationNational {
///     pub country: String,
///     pub main_classification: String,
/// }
fn deser_class_national<B: BufRead>(
    rdr: &mut quick_xml::Reader<B>,
    buf: &mut Vec<u8>,
    class_national: &mut ClassificationNational
    ) -> Result<(), Error>
{
    parse_struct_update_from!(
        rdr,
        buf,
        "classification-national",
        class_national,
        // Required
        {
            b"country" => country,
            b"additional-info" => additional_info,
            b"main-classification" => main_classification,
        },
        // Optional
        {
            b"further-classification" => further_classification,
        }
    );

    Ok(())
}

fn deser_text<B: BufRead, K: AsRef<[u8]>>(end: K, rdr: &mut quick_xml::Reader<B>) -> Result<String, Error> {
    match rdr.read_text(end, &mut Vec::new()) {
        Ok(txt) => Ok(txt),
        Err(err) => Err(Error::Deser { src: err.to_string() }),
    }
}

