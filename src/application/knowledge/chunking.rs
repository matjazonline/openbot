//! Pure heading/paragraph packing with an injected model token counter.
use super::{
    MAX_CHUNKS_PER_FILE, MAX_EMBEDDING_TOKENS, MAX_HEADING_BYTES, MAX_HEADING_TOKENS,
    TARGET_CHUNK_TOKENS,
};
use crate::{
    app_error::{AppError, AppResult},
    entities::knowledge::MAX_KNOWLEDGE_FILE_BYTES,
};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeChunk {
    pub heading: Option<String>,
    pub body: String,
}
impl KnowledgeChunk {
    pub fn embedding_text(&self) -> String {
        render(self.heading.as_deref(), &self.body)
    }
}
fn render(heading: Option<&str>, body: &str) -> String {
    match heading {
        Some(heading) if body.is_empty() => heading.to_owned(),
        Some(heading) => format!("{heading}\n\n{body}"),
        None => body.to_owned(),
    }
}
fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}
struct Heading {
    start: usize,
    end: usize,
    level: usize,
    title: String,
}
struct HeadingPart {
    level: usize,
    title: String,
}
fn headings(source: &str) -> AppResult<Vec<Heading>> {
    let mut found = Vec::new();
    let mut current: Option<Heading> = None;
    for (event, range) in Parser::new(source).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                current = Some(Heading {
                    start: range.start,
                    end: range.end,
                    level: level as usize,
                    title: String::new(),
                })
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(heading) = &mut current {
                    heading.title.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(heading) = &mut current {
                    heading.title.push(' ');
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(mut heading) = current.take() {
                    heading.end = range.end;
                    if found.len() == MAX_CHUNKS_PER_FILE {
                        return Err(invalid("The file has too many sections"));
                    }
                    found.push(heading);
                }
            }
            _ => {}
        }
    }
    Ok(found)
}

pub fn chunk_document(
    source: &str,
    mut count: impl FnMut(&str) -> AppResult<usize>,
) -> AppResult<Vec<KnowledgeChunk>> {
    if source.len() > MAX_KNOWLEDGE_FILE_BYTES {
        return Err(invalid("The file exceeds 1 MiB"));
    }
    let markers = headings(source)?;
    let mut packer = Packer {
        count: &mut count,
        chunks: Vec::new(),
        heading: None,
        pending: String::new(),
    };
    let mut path = Vec::new();
    let mut cursor = 0;
    for marker in markers {
        packer.section(&source[cursor..marker.start])?;
        while path
            .last()
            .is_some_and(|part: &HeadingPart| part.level >= marker.level)
        {
            path.pop();
        }
        path.push(HeadingPart {
            level: marker.level,
            title: marker.title,
        });
        let heading = path
            .iter()
            .map(|part| part.title.as_str())
            .collect::<Vec<_>>()
            .join(" / ");
        if heading.len() > MAX_HEADING_BYTES || (packer.count)(&heading)? > MAX_HEADING_TOKENS {
            return Err(invalid(
                "A section heading is too long; use shorter section headings",
            ));
        }
        packer.heading = Some(heading);
        cursor = marker.end;
    }
    packer.section(&source[cursor..])?;
    if packer.chunks.is_empty() {
        packer.push_body(String::new())?;
    }
    Ok(packer.chunks)
}
struct Packer<'a, F> {
    count: &'a mut F,
    chunks: Vec<KnowledgeChunk>,
    heading: Option<String>,
    pending: String,
}
impl<F: FnMut(&str) -> AppResult<usize>> Packer<'_, F> {
    fn tokens(&mut self, body: &str) -> AppResult<usize> {
        (self.count)(&render(self.heading.as_deref(), body))
    }
    fn push_body(&mut self, body: String) -> AppResult<()> {
        if self.chunks.len() == MAX_CHUNKS_PER_FILE {
            return Err(invalid("The file has too many chunks"));
        }
        if self.tokens(&body)? > MAX_EMBEDDING_TOKENS {
            return Err(invalid("A text fragment exceeds the model limit"));
        }
        self.chunks.push(KnowledgeChunk {
            heading: self.heading.clone(),
            body,
        });
        Ok(())
    }
    fn flush(&mut self) -> AppResult<()> {
        if !self.pending.is_empty() {
            let body = std::mem::take(&mut self.pending);
            self.push_body(body)?;
        }
        Ok(())
    }
    fn section(&mut self, body: &str) -> AppResult<()> {
        let before = self.chunks.len();
        let mut paragraph_start = 0;
        let mut offset = 0;
        for line in body.split_inclusive('\n') {
            let end = offset + line.len();
            if line.trim().is_empty() {
                let paragraph = body[paragraph_start..offset].trim_matches(['\r', '\n']);
                if !paragraph.is_empty() {
                    self.paragraph(paragraph)?;
                }
                paragraph_start = end;
            }
            offset = end;
        }
        let remaining = body[paragraph_start..].trim_matches(['\r', '\n']);
        if !remaining.is_empty() {
            self.paragraph(remaining)?;
        }
        self.flush()?;
        if self.chunks.len() == before && self.heading.is_some() {
            self.push_body(String::new())?;
        }
        Ok(())
    }
    fn paragraph(&mut self, mut remaining: &str) -> AppResult<()> {
        while !remaining.is_empty() {
            let candidate = if self.pending.is_empty() {
                remaining.to_owned()
            } else {
                format!("{}\n\n{remaining}", self.pending)
            };
            if self.tokens(&candidate)? <= TARGET_CHUNK_TOKENS {
                self.pending = candidate;
                break;
            }
            if !self.pending.is_empty() {
                self.flush()?;
                continue;
            }
            if self.tokens(remaining)? <= MAX_EMBEDDING_TOKENS {
                self.pending.push_str(remaining);
                break;
            }
            let bytes = self.prefix(remaining)?;
            self.push_body(remaining[..bytes].to_owned())?;
            remaining = &remaining[bytes..];
        }
        Ok(())
    }
    fn prefix(&mut self, text: &str) -> AppResult<usize> {
        let boundaries: Vec<usize> = text
            .char_indices()
            .map(|(at, _)| at)
            .chain(std::iter::once(text.len()))
            .collect();
        let mut low = 1;
        let mut high = boundaries.len() - 1;
        let mut best = 0;
        while low <= high {
            let mid = low + (high - low) / 2;
            let bytes = boundaries[mid];
            if self.tokens(&text[..bytes])? <= TARGET_CHUNK_TOKENS {
                best = bytes;
                low = mid + 1;
            } else {
                high = mid - 1;
            }
        }
        if best == 0 {
            return Err(invalid(
                "A text fragment cannot fit with its section heading",
            ));
        }
        Ok(best)
    }
}

#[cfg(test)]
#[path = "chunking_tests.rs"]
mod tests;
