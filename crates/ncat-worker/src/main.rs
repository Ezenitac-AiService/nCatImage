#![forbid(unsafe_code)]

use ncat_media::Book;
use ncat_protocol::{Envelope, ErrorCode, Page, Request, Response, read_message, write_message};
use std::io::{self, BufReader, BufWriter, Write};

fn pages(writer: &mut impl Write, id: u64, book: &Book) -> io::Result<()> {
    let mut chunk: Vec<Page> = Vec::new();
    let mut cost = 0;
    for page in &book.pages {
        if !chunk.is_empty()
            && (cost + page.cost() > ncat_protocol::PAGE_BATCH_BYTES
                || chunk.len() >= ncat_protocol::PAGE_BATCH_ENTRIES)
        {
            write_message(
                writer,
                &Envelope::new(
                    id,
                    Response::Pages {
                        pages: std::mem::take(&mut chunk),
                        selected: book.selected,
                        done: false,
                    },
                ),
            )?;
            cost = 0;
        }
        cost += page.cost();
        chunk.push(page.clone());
    }
    write_message(
        writer,
        &Envelope::new(
            id,
            Response::Pages {
                pages: chunk,
                selected: book.selected,
                done: true,
            },
        ),
    )
}

fn run() -> io::Result<()> {
    let mut input = BufReader::new(io::stdin().lock());
    let mut output = BufWriter::new(io::stdout().lock());
    let mut book: Option<Book> = None;
    while let Some(message) = read_message::<Request>(&mut input)? {
        let result = match message.body {
            Request::Open { source } => {
                book = None;
                match Book::open(&source.path()) {
                    Ok(next) => {
                        write_message(
                            &mut output,
                            &Envelope::new(
                                message.id,
                                Response::Opened {
                                    title: next.title.clone(),
                                    deferred: next.deferred,
                                },
                            ),
                        )?;
                        pages(&mut output, message.id, &next)?;
                        book = Some(next);
                        Ok(None)
                    }
                    Err(error) => Err(error),
                }
            }
            Request::Catalog => match book.as_mut() {
                Some(book) => match book.catalog() {
                    Ok(()) => {
                        pages(&mut output, message.id, book)?;
                        Ok(None)
                    }
                    Err(e) => Err(e),
                },
                None => Err(ErrorCode::Protocol),
            },
            Request::Decode {
                key,
                output,
                max_width,
                max_height,
                workspace,
                quarter_turns,
            } => book
                .as_ref()
                .ok_or(ErrorCode::Protocol)
                .and_then(|book| {
                    book.decode(
                        &key,
                        &output.path(),
                        max_width,
                        max_height,
                        workspace,
                        quarter_turns,
                    )
                })
                .map(|frame| Some(Response::Frame(frame))),
            Request::Shutdown => {
                write_message(&mut output, &Envelope::new(message.id, Response::Bye))?;
                break;
            }
        };
        if let Some(response) = match result {
            Ok(response) => response,
            Err(e) => Some(Response::Error(e)),
        } {
            write_message(&mut output, &Envelope::new(message.id, response))?;
        }
    }
    Ok(())
}

fn main() {
    let protected = ncat_platform::protect_worker().is_ok();
    std::panic::set_hook(Box::new(|_| eprintln!("ncat-worker: internal failure")));
    if !protected {
        eprintln!("ncat-worker: resource protection unavailable");
        std::process::exit(2);
    }
    if run().is_err() {
        eprintln!("ncat-worker: protocol failure");
        std::process::exit(1);
    }
}
