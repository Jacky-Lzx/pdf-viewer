//! Standard PDF markup annotations. Geometry is kept in renderer coordinates.
#[path = "annotations/geometry.rs"]
mod geometry;

use hayro::{
    hayro_interpret::util::TransformExt,
    hayro_syntax::{
        Pdf,
        object::{
            Array, Dict, MaybeRef, Name, ObjRef, Object, Rect as PdfRect, String as PdfString,
        },
    },
    kurbo::Rect,
};
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CommentId {
    pub page: usize,
    pub slot: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReviewStatus {
    #[default]
    None,
    Accepted,
    Rejected,
    Cancelled,
    Completed,
}

impl ReviewStatus {
    pub const ALL: [Self; 5] = [
        Self::None,
        Self::Accepted,
        Self::Rejected,
        Self::Cancelled,
        Self::Completed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Completed => "Completed / resolved",
            _ => self.pdf_value(),
        }
    }

    fn pdf_value(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Accepted => "Accepted",
            Self::Rejected => "Rejected",
            Self::Cancelled => "Cancelled",
            Self::Completed => "Completed",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.pdf_value() == value)
    }
}

/// State changes are separate Text annotations, often themselves in a reply chain.
#[derive(Clone, Debug)]
pub struct CommentState {
    pub id: CommentId,
    pub author: String,
    pub model: String,
    pub value: String,
    depth: usize,
}

#[derive(Clone, Debug)]
pub struct Comment {
    pub page: usize,
    /// Raw /Annots slot; None creates a new highlight or reply.
    pub slot: Option<usize>,
    pub parent: Option<CommentId>,
    pub review: ReviewStatus,
    pub marked: bool,
    pub states: Vec<CommentState>,
    pub kind: String,
    pub contents: String,
    pub author: String,
    pub rects: Vec<Rect>,
    pub has_appearance: bool,
    pub truncated: bool,
    pub locked: bool,
}

impl Comment {
    pub fn id(&self) -> Option<CommentId> {
        self.slot.map(|slot| CommentId {
            page: self.page,
            slot,
        })
    }

    pub fn reply(&self) -> Option<Self> {
        Some(Self {
            page: self.page,
            slot: None,
            parent: Some(self.id()?),
            kind: "Text".into(),
            contents: String::new(),
            author: "Pagewing".into(),
            rects: self.rects.clone(),
            has_appearance: false,
            truncated: false,
            locked: false,
            review: ReviewStatus::None,
            marked: false,
            states: Vec::new(),
        })
    }

    pub fn editable(&self) -> bool {
        !self.locked && !self.truncated && self.kind != "FreeText"
    }
}

#[derive(Default)]
pub struct Comments {
    pub items: Vec<Comment>,
    pub limited: bool,
}

impl Comments {
    /// Iterative preorder keeps arbitrary nesting and malformed cycles stack safe.
    pub fn thread_rows(&self) -> Vec<(usize, usize)> {
        if self.items.iter().all(|comment| comment.parent.is_none()) {
            return (0..self.items.len()).map(|index| (index, 0)).collect();
        }
        let mut indices = HashMap::with_capacity(self.items.len());
        indices.extend(
            self.items
                .iter()
                .enumerate()
                .filter_map(|(i, c)| Some((c.id()?, i))),
        );
        // Link siblings in reverse source order so the LIFO traversal visits
        // them in source order, without allocating a child list per parent.
        const NONE: usize = usize::MAX;
        let mut last_child = vec![NONE; self.items.len()];
        let mut previous_sibling = vec![NONE; self.items.len()];
        let mut roots = Vec::new();
        for (index, comment) in self.items.iter().enumerate() {
            if let Some(parent) = comment
                .parent
                .and_then(|p| indices.get(&p))
                .copied()
                .filter(|p| *p != index)
            {
                previous_sibling[index] = last_child[parent];
                last_child[parent] = index;
            } else {
                roots.push(index);
            }
        }
        let mut rows = Vec::with_capacity(self.items.len());
        let mut visited = vec![false; self.items.len()];
        let mut stack = Vec::new();
        for root in roots.into_iter().chain(0..self.items.len()) {
            if visited[root] {
                continue;
            }
            // Each traversal drains the stack. Reuse its storage for the next
            // root, including the fallback roots that expose malformed cycles.
            stack.push((root, 0));
            while let Some((index, depth)) = stack.pop() {
                if std::mem::replace(&mut visited[index], true) {
                    continue;
                }
                rows.push((index, depth));
                let mut child = last_child[index];
                while child != NONE {
                    stack.push((child, depth + 1));
                    child = previous_sibling[child];
                }
            }
        }
        rows
    }
}

pub fn read(pdf: &Pdf, cancelled: impl Fn() -> bool) -> Comments {
    let mut result = Comments::default();
    let mut budget = 100_000usize;
    let mut references = HashMap::new();
    let mut parents = Vec::new();
    let mut states = Vec::new();
    let mut state_links = HashMap::new();
    'pages: for (page_index, page) in pdf.pages().iter().enumerate() {
        if cancelled() {
            break;
        }
        let Some(array) = page.raw().get::<Array<'_>>(b"Annots") else {
            continue;
        };
        let transform = page.initial_transform(true).to_kurbo();
        for (slot, object) in array.raw_iter().enumerate() {
            if budget == 0 || result.items.len() >= 10_000 {
                result.limited = true;
                break 'pages;
            }
            budget -= 1;
            if cancelled() {
                break 'pages;
            }
            let reference = match &object {
                MaybeRef::Ref(id) => Some(*id),
                _ => None,
            };
            let dict = match object {
                MaybeRef::Ref(id) => pdf.xref().get::<Dict<'_>>(id.into()),
                MaybeRef::NotRef(Object::Dict(dict)) => Some(dict),
                _ => None,
            };
            let Some(dict) = dict else { continue };
            let Some(kind) = dict.get::<Name<'_>>(b"Subtype") else {
                continue;
            };
            let id = CommentId {
                page: page_index,
                slot,
            };
            let author = pdf_text(&dict, b"T", 1024);
            let parent = dict.get_ref(b"IRT");
            let model = pdf_text(&dict, b"StateModel", 64);
            if kind.as_ref() == b"Text" && !model.is_empty() {
                let value = dict
                    .get::<PdfString<'_>>(b"State")
                    .map(|s| crate::pdf_string::decode_prefix(s.as_bytes(), 64).0)
                    .unwrap_or_else(|| {
                        if model == "Marked" {
                            "Unmarked"
                        } else {
                            "None"
                        }
                        .into()
                    });
                if let Some(reference) = reference {
                    state_links.insert(reference, parent);
                }
                states.push((
                    parent,
                    CommentState {
                        id,
                        author,
                        model,
                        value,
                        depth: 0,
                    },
                ));
                continue; // State records may be hidden and are never discussion replies.
            }
            if !matches!(
                kind.as_ref(),
                b"Highlight" | b"Text" | b"Underline" | b"StrikeOut" | b"Squiggly" | b"FreeText"
            ) {
                continue;
            }
            let flags = dict.get::<u32>(b"F").unwrap_or(0);
            if flags & (1 | 2 | 32) != 0 {
                continue;
            }
            let (contents, truncated) = dict
                .get::<PdfString<'_>>(b"Contents")
                .map(|s| crate::pdf_string::decode_prefix(s.as_bytes(), 65_536))
                .unwrap_or_default();
            let mut rects = Vec::new();
            if let Some(quads) = dict.get::<Array<'_>>(b"QuadPoints") {
                rects = geometry::quad_rects(quads.iter::<f32>(), transform);
            }
            if rects.is_empty()
                && let Some(r) = dict.get::<PdfRect>(b"Rect")
            {
                let r = transform.transform_rect_bbox(Rect::new(r.x0, r.y0, r.x1, r.y1));
                if r.is_finite() && r.width() > 0.0 && r.height() > 0.0 {
                    rects.push(r);
                }
            }
            if let Some(reference) = reference {
                references.entry(reference).or_insert(id);
            }
            parents.push(
                if dict
                    .get::<Name<'_>>(b"RT")
                    .is_some_and(|n| n.as_ref() == b"Group")
                {
                    None
                } else {
                    parent
                },
            );
            result.items.push(Comment {
                page: page_index,
                slot: Some(slot),
                parent: None,
                review: ReviewStatus::None,
                marked: false,
                states: Vec::new(),
                kind: String::from_utf8_lossy(kind.as_ref()).into_owned(),
                contents,
                author,
                rects,
                truncated,
                has_appearance: dict.get::<Dict<'_>>(b"AP").is_some(),
                locked: flags & (64 | 128 | 512) != 0,
            });
        }
    }
    let indices: HashMap<_, _> = result
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, c)| Some((c.id()?, i)))
        .collect();
    for (comment, parent) in result.items.iter_mut().zip(parents) {
        comment.parent = parent
            .and_then(|p| references.get(&p).copied())
            .filter(|p| Some(*p) != comment.id());
    }
    // Resolve each state chain once. Broken and cyclic IRT links remain inert.
    let mut resolved: HashMap<ObjRef, Option<(CommentId, usize)>> = HashMap::new();
    for (parent, mut state) in states {
        let mut path = Vec::new();
        let mut cursor = parent;
        let mut visiting = std::collections::HashSet::new();
        let target = loop {
            let Some(reference) = cursor else { break None };
            if let Some(id) = references.get(&reference) {
                break Some((*id, 0));
            }
            if let Some(target) = resolved.get(&reference) {
                break *target;
            }
            if !visiting.insert(reference) {
                break None;
            }
            path.push(reference);
            cursor = state_links.get(&reference).copied().flatten();
        };
        let mut target = target;
        for reference in path.into_iter().rev() {
            target = target.map(|(id, depth)| (id, depth + 1));
            resolved.insert(reference, target);
        }
        if let Some((id, depth)) = target
            && let Some(&index) = indices.get(&id)
        {
            state.depth = depth;
            result.items[index].states.push(state);
        }
    }
    for comment in &mut result.items {
        // Prefer our own user's state; retain every other author's history for display.
        comment
            .states
            .sort_by_key(|s| (s.author == "Pagewing", s.depth, s.id.page, s.id.slot));
        for state in &comment.states {
            if state.model == "Review"
                && let Some(review) = ReviewStatus::parse(&state.value)
            {
                comment.review = review;
            }
            if state.model == "Marked" {
                match state.value.as_str() {
                    "Marked" => comment.marked = true,
                    "Unmarked" => comment.marked = false,
                    _ => {}
                }
            }
        }
    }
    result
}

fn pdf_text(dict: &Dict<'_>, key: &[u8], limit: usize) -> String {
    dict.get::<PdfString<'_>>(key)
        .map(|s| crate::pdf_string::decode_prefix(s.as_bytes(), limit).0)
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Save,
    Delete,
}

/// Save an incremental PDF update atomically over the current document.
pub fn save(pdf: &Pdf, comment: &Comment, source: &Path) -> Result<(), String> {
    write(pdf, comment, source, Operation::Save)
}

/// Remove the selected annotation, its popup, replies and status history.
pub fn delete(pdf: &Pdf, comment: &Comment, source: &Path) -> Result<(), String> {
    write(pdf, comment, source, Operation::Delete)
}

fn write(pdf: &Pdf, comment: &Comment, source: &Path, operation: Operation) -> Result<(), String> {
    let source = source
        .canonicalize()
        .map_err(|e| format!("Cannot locate PDF: {e}"))?;
    let permissions = std::fs::metadata(&source)
        .map_err(|e| format!("Cannot inspect PDF: {e}"))?
        .permissions();
    if permissions.readonly() {
        return Err("This PDF is read-only.".into());
    }
    check_current(pdf, &source)?;
    let bytes = pdf.data().as_ref().to_vec();
    let previous =
        lopdf::Document::load_mem(&bytes).map_err(|e| format!("Cannot edit PDF: {e}"))?;
    if previous.is_encrypted() {
        return Err(
            "Saving comments in encrypted PDFs is not supported. Save an unencrypted copy first."
                .into(),
        );
    }
    let original = if comment.slot.is_some() && operation == Operation::Save {
        read(pdf, || false)
            .items
            .into_iter()
            .find(|c| c.id() == comment.id())
    } else {
        None
    };
    let contents_changed = original
        .as_ref()
        .is_none_or(|c| c.contents != comment.contents);
    if comment.locked || (operation == Operation::Save && contents_changed && !comment.editable()) {
        return Err("This annotation is read-only.".into());
    }
    let page = pdf
        .pages()
        .get(comment.page)
        .ok_or("Comment page is out of range")?;
    let page_id = *previous
        .get_pages()
        .get(&(comment.page as u32 + 1))
        .ok_or("Missing PDF page")?;
    let mut update = lopdf::IncrementalDocument::create_from(bytes, previous);
    let previous = update.get_prev_documents();
    let page_dict = previous
        .get_dictionary(page_id)
        .map_err(|e| e.to_string())?;
    let mut annots = match page_dict.get(b"Annots") {
        Ok(value) => previous
            .dereference(value)
            .and_then(|(_, value)| value.as_array())
            .map_err(|e| format!("Cannot read page annotations: {e}"))?
            .clone(),
        Err(_) => Vec::new(),
    };
    use lopdf::{Object as O, dictionary};
    if operation == Operation::Delete {
        let id = comment
            .id()
            .ok_or("This comment has not been saved. Use Discard to remove the draft.")?;
        annots = delete_thread(&mut update, id)?;
    } else if let Some(slot) = comment.slot {
        let value = annots.get(slot).ok_or("Annotation no longer exists")?;
        let mut dict = previous
            .dereference(value)
            .and_then(|(_, v)| v.as_dict())
            .map_err(|e| e.to_string())?
            .clone();
        if contents_changed {
            dict.set("Contents", lopdf::text_string(&comment.contents));
            dict.remove(b"RC"); // Plain text is now authoritative.
        }
        let id = if let O::Reference(id) = value {
            *id
        } else {
            update.new_document.new_object_id()
        };
        update.new_document.set_object(id, dict);
        annots[slot] = O::Reference(id);
    } else if let Some(parent) = comment.parent {
        if parent.page != comment.page {
            return Err("Replies must be on the same page as their parent.".into());
        }
        if comment.contents.trim().is_empty() {
            return Err("Write a reply before saving.".into());
        }
        let parent_id = annotation_reference(&mut update, &mut annots, parent.slot)?;
        let parent_dict = update
            .new_document
            .get_dictionary(parent_id)
            .or_else(|_| update.get_prev_documents().get_dictionary(parent_id))
            .map_err(|e| e.to_string())?;
        if parent_dict.get(b"F").and_then(O::as_i64).unwrap_or(0) & (64 | 128 | 512) != 0 {
            return Err("This annotation is read-only.".into());
        }
        let rect = parent_dict
            .get(b"Rect")
            .cloned()
            .unwrap_or_else(|_| O::Array(vec![0.into(); 4]));
        let id = update.new_document.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Text", "IRT" => parent_id, "RT" => "R",
            "Rect" => rect, "Contents" => lopdf::text_string(&comment.contents),
            "T" => lopdf::text_string("Pagewing"), "F" => 4, "P" => page_id,
        });
        annots.push(O::Reference(id));
    } else {
        if comment.rects.is_empty() {
            return Err("Select some text first.".into());
        }
        let inverse = page.initial_transform(true).to_kurbo().inverse();
        let rects: Vec<_> = comment
            .rects
            .iter()
            .map(|r| inverse.transform_rect_bbox(*r))
            .collect();
        if rects
            .iter()
            .any(|r| !r.is_finite() || r.width() <= 0.0 || r.height() <= 0.0)
        {
            return Err("Invalid highlight geometry.".into());
        }
        let bounds = rects.iter().copied().reduce(|a, b| a.union(b)).unwrap();
        let array =
            |values: Vec<f64>| O::Array(values.into_iter().map(|v| O::Real(v as f32)).collect());
        let bbox = || array(vec![bounds.x0, bounds.y0, bounds.x1, bounds.y1]);
        let quads = array(
            rects
                .iter()
                .flat_map(|r| [r.x0, r.y1, r.x1, r.y1, r.x0, r.y0, r.x1, r.y0])
                .collect(),
        );
        let mut appearance = String::from("q /GS gs 1 0.85 0 rg\n");
        use std::fmt::Write;
        for r in &rects {
            let _ = writeln!(
                appearance,
                "{} {} {} {} re f",
                r.x0,
                r.y0,
                r.width(),
                r.height()
            );
        }
        appearance.push_str("Q\n");
        let stream = lopdf::Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form", "BBox" => bbox(),
                "Resources" => dictionary! { "ExtGState" => dictionary! { "GS" => dictionary! { "Type" => "ExtGState", "BM" => "Multiply", "ca" => 0.35f32 } } },
            },
            appearance.into_bytes(),
        );
        let appearance_id = update.new_document.add_object(stream);
        let id = update.new_document.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Highlight", "Rect" => bbox(), "QuadPoints" => quads,
            "Contents" => lopdf::text_string(&comment.contents), "T" => lopdf::text_string("Pagewing"),
            "C" => array(vec![1.0, 0.85, 0.0]), "CA" => 0.35f32, "F" => 4,
            "AP" => dictionary! { "N" => appearance_id }, "P" => page_id,
        });
        annots.push(O::Reference(id));
    }
    if operation == Operation::Save {
        let review_changed = original
            .as_ref()
            .map_or(comment.review != ReviewStatus::None, |c| {
                c.review != comment.review
            });
        let marked_changed = original
            .as_ref()
            .map_or(comment.marked, |c| c.marked != comment.marked);
        if review_changed || marked_changed {
            let slot = comment.slot.unwrap_or(annots.len() - 1);
            let target = annotation_reference(&mut update, &mut annots, slot)?;
            for (changed, model, value) in [
                (review_changed, "Review", comment.review.pdf_value()),
                (
                    marked_changed,
                    "Marked",
                    if comment.marked { "Marked" } else { "Unmarked" },
                ),
            ] {
                if !changed {
                    continue;
                }
                let previous_state = original.as_ref().and_then(|c| {
                    c.states
                        .iter()
                        .rev()
                        .find(|s| s.author == "Pagewing" && s.model == model)
                });
                let parent = if let Some(state) = previous_state {
                    if state.id.page == comment.page {
                        annotation_reference(&mut update, &mut annots, state.id.slot)?
                    } else {
                        let page =
                            update.get_prev_documents().get_pages()[&(state.id.page as u32 + 1)];
                        let entries = page_annotations(update.get_prev_documents(), page)?;
                        entries
                            .get(state.id.slot)
                            .and_then(|v| v.as_reference().ok())
                            .unwrap_or(target)
                    }
                } else {
                    target
                };
                let id = update.new_document.add_object(dictionary! {
                    "Type" => "Annot", "Subtype" => "Text", "IRT" => parent, "RT" => "R",
                    "Rect" => vec![O::Integer(0); 4], "F" => 2, "P" => page_id,
                    "T" => lopdf::text_string("Pagewing"),
                    "StateModel" => lopdf::text_string(model), "State" => lopdf::text_string(value),
                });
                annots.push(O::Reference(id));
            }
        }
    }
    update
        .opt_clone_object_to_new_document(page_id)
        .map_err(|e| e.to_string())?;
    update
        .new_document
        .get_dictionary_mut(page_id)
        .map_err(|e| e.to_string())?
        .set("Annots", annots);
    let parent = source
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp =
        tempfile::NamedTempFile::new_in(parent).map_err(|e| format!("Cannot create PDF: {e}"))?;
    update
        .save_to(temp.as_file_mut())
        .map_err(|e| format!("Cannot save PDF: {e}"))?;
    temp.as_file()
        .set_permissions(permissions)
        .map_err(|e| format!("Cannot preserve PDF permissions: {e}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|e| format!("Cannot flush PDF: {e}"))?;
    check_current(pdf, &source)?;
    temp.persist(&source)
        .map_err(|e| format!("Cannot finish saving PDF: {e}"))?;
    Ok(())
}

fn page_annotations(
    document: &lopdf::Document,
    page: lopdf::ObjectId,
) -> Result<Vec<lopdf::Object>, String> {
    match document
        .get_dictionary(page)
        .map_err(|e| e.to_string())?
        .get(b"Annots")
    {
        Ok(value) => document
            .dereference(value)
            .and_then(|(_, v)| v.as_array())
            .cloned()
            .map_err(|e| e.to_string()),
        Err(_) => Ok(Vec::new()),
    }
}

/// Promote a direct annotation without changing its raw slot or existing objects.
fn annotation_reference(
    update: &mut lopdf::IncrementalDocument,
    annots: &mut [lopdf::Object],
    slot: usize,
) -> Result<lopdf::ObjectId, String> {
    use lopdf::Object;
    let value = annots.get_mut(slot).ok_or("Annotation no longer exists")?;
    if let Object::Reference(id) = value {
        return Ok(*id);
    }
    let dict = value.as_dict().map_err(|e| e.to_string())?.clone();
    let id = update.new_document.add_object(dict);
    *value = Object::Reference(id);
    Ok(id)
}

fn delete_thread(
    update: &mut lopdf::IncrementalDocument,
    selected: CommentId,
) -> Result<Vec<lopdf::Object>, String> {
    let previous = update.get_prev_documents();
    let mut entries = Vec::new();
    let mut ranges = Vec::new();
    let mut selected_slot = None;
    for (number, page) in previous.get_pages() {
        let annots = page_annotations(previous, page)?;
        if number as usize == selected.page + 1 {
            if selected.slot >= annots.len() {
                return Err("Annotation no longer exists".into());
            }
            selected_slot = Some(entries.len() + selected.slot);
        }
        ranges.push((number, page, entries.len(), annots.len()));
        entries.extend(annots);
    }
    let removed = removed_comments(previous, &entries, selected_slot.ok_or("Missing PDF page")?)?;
    let mut selected_annots = Vec::new();
    for (number, page, start, len) in ranges {
        let annots: Vec<_> = (start..start + len)
            .filter(|&i| !removed[i])
            .map(|i| entries[i].clone())
            .collect();
        if number as usize == selected.page + 1 {
            selected_annots = annots.clone();
        }
        if annots.len() != len {
            update
                .opt_clone_object_to_new_document(page)
                .map_err(|e| e.to_string())?;
            update
                .new_document
                .get_dictionary_mut(page)
                .map_err(|e| e.to_string())?
                .set("Annots", annots);
        }
    }
    Ok(selected_annots)
}

/// Preserve raw /Annots entries, including unrelated and malformed objects.
/// A work queue follows popup/reply links across pages without repeated scans.
fn removed_comments(
    document: &lopdf::Document,
    annots: &[lopdf::Object],
    slot: usize,
) -> Result<Vec<bool>, String> {
    use lopdf::{Object, ObjectId};
    use std::collections::{HashMap, VecDeque};
    let selected = annots.get(slot).ok_or("Annotation no longer exists")?;
    document
        .dereference(selected)
        .and_then(|(_, value)| value.as_dict())
        .map_err(|e| format!("Cannot delete annotation: {e}"))?;
    let mut references: HashMap<ObjectId, Vec<usize>> = HashMap::new();
    let mut children: HashMap<ObjectId, Vec<usize>> = HashMap::new();
    let mut popups = vec![None; annots.len()];
    for (index, value) in annots.iter().enumerate() {
        if let Ok(id) = value.as_reference() {
            references.entry(id).or_default().push(index);
        }
        let Ok((_, value)) = document.dereference(value) else {
            continue;
        };
        let Ok(dict) = value.as_dict() else { continue };
        popups[index] = dict.get(b"Popup").and_then(Object::as_reference).ok();
        if let Ok(parent) = dict.get(b"IRT").and_then(Object::as_reference) {
            children.entry(parent).or_default().push(index);
        }
        if dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Popup")
            && let Ok(parent) = dict.get(b"Parent").and_then(Object::as_reference)
        {
            children.entry(parent).or_default().push(index);
        }
    }
    let mut removed = vec![false; annots.len()];
    let mut queue = VecDeque::from([slot]);
    while let Some(index) = queue.pop_front() {
        if std::mem::replace(&mut removed[index], true) {
            continue;
        }
        if let Ok(id) = annots[index].as_reference() {
            queue.extend(references.remove(&id).unwrap_or_default());
            queue.extend(children.remove(&id).unwrap_or_default());
        }
        if let Some(popup) = popups[index] {
            queue.extend(references.remove(&popup).unwrap_or_default());
        }
    }
    Ok(removed)
}

/// Never replace a file which another program changed since it was loaded.
fn check_current(pdf: &Pdf, source: &Path) -> Result<(), String> {
    let current = std::fs::read(source).map_err(|e| format!("Cannot read current PDF: {e}"))?;
    if current != pdf.data().as_ref() {
        return Err("The PDF changed on disk. Reload it before saving comments.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Object as O, dictionary};

    fn draft() -> Comment {
        Comment {
            page: 0,
            slot: None,
            parent: None,
            review: Default::default(),
            marked: false,
            states: Vec::new(),
            kind: "Highlight".into(),
            contents: "中文批注 📝\nSecond line".into(),
            author: "Pagewing".into(),
            rects: vec![
                Rect::new(75.0, 90.0, 210.0, 104.0),
                Rect::new(75.0, 110.0, 240.0, 124.0),
            ],
            has_appearance: false,
            truncated: false,
            locked: false,
        }
    }

    fn source(root: &Path) -> (std::path::PathBuf, Pdf) {
        let path = root.join("source.pdf");
        std::fs::write(&path, include_bytes!("../assets/pagewing-guide.pdf")).unwrap();
        let pdf = crate::document::load(&path, "").unwrap();
        (path, pdf)
    }

    #[test]
    fn quad_points_preserve_cropped_rotated_geometry_and_rect_fallbacks() {
        for rotation in [0, 90, 180, 270] {
            for complete in [true, false] {
                let mut document =
                    lopdf::Document::load_mem(include_bytes!("../assets/pagewing-guide.pdf"))
                        .unwrap();
                let page_id = document.get_pages()[&1];
                let mut numbers: Vec<O> = if complete {
                    [
                        50, 600, 150, 600, 50, 620, 150, 620, 70, 570, 180, 570, 70, 590, 180, 590,
                    ]
                    .into_iter()
                    .map(O::Integer)
                    .collect()
                } else {
                    vec![O::Integer(0); 16]
                };
                // Seven trailing coordinates must not create a partial region.
                numbers.extend([10, 20, 30, 40, 50, 60, 70].into_iter().map(O::Integer));
                let annotation = dictionary! { "Subtype" => "Highlight", "Contents" => "Multiple lines", "Rect" => vec![40.into(), 500.into(), 200.into(), 650.into()], "QuadPoints" => numbers };
                let page = document.get_dictionary_mut(page_id).unwrap();
                page.set(
                    "CropBox",
                    vec![20.into(), 30.into(), 550.into(), 730.into()],
                );
                page.set("Rotate", rotation);
                page.set("Annots", vec![O::Dictionary(annotation)]);
                let mut bytes = Vec::new();
                document.save_to(&mut bytes).unwrap();
                let pdf = Pdf::new(bytes).unwrap();
                let transform = pdf.pages()[0].initial_transform(true).to_kurbo();
                let expected = if complete {
                    vec![
                        Rect::new(50.0, 600.0, 150.0, 620.0),
                        Rect::new(70.0, 570.0, 180.0, 590.0),
                    ]
                } else {
                    vec![Rect::new(40.0, 500.0, 200.0, 650.0)]
                };
                let comments = read(&pdf, || false);
                assert_eq!(comments.items.len(), 1);
                assert_eq!(
                    comments.items[0].rects,
                    expected
                        .into_iter()
                        .map(|rect| transform.transform_rect_bbox(rect))
                        .collect::<Vec<_>>(),
                    "rotation {rotation}, complete {complete}"
                );
            }
        }
    }

    #[test]
    fn in_place_comment_roundtrip_preserves_links_text_and_renders_appearance() {
        let root = tempfile::tempdir().unwrap();
        let (source, pdf) = source(root.path());
        let comment = draft();
        save(&pdf, &comment, &source).unwrap();
        let bytes = std::fs::read(&source).unwrap();
        assert!(
            bytes.starts_with(pdf.data().as_ref()),
            "incremental save must retain original bytes"
        );
        assert_ne!(std::fs::read(&source).unwrap(), pdf.data().as_ref());
        assert_eq!(
            std::fs::read_dir(root.path()).unwrap().count(),
            1,
            "saving must not create a copy or backup"
        );
        let saved = crate::document::load(&source, "").unwrap();
        let comments = read(&saved, || false);
        assert_eq!(comments.items.len(), 1);
        let actual = &comments.items[0];
        assert_eq!(actual.contents, comment.contents);
        assert_eq!(actual.rects, comment.rects);
        assert!(actual.has_appearance);
        let interpreter = hayro::hayro_interpret::InterpreterCache::new();
        assert_eq!(
            crate::text::extract(&pdf.pages()[0], &interpreter).text,
            crate::text::extract(&saved.pages()[0], &interpreter).text
        );
        assert_eq!(
            crate::links::LinkReader::new(&pdf)
                .read(&pdf.pages()[0])
                .len(),
            crate::links::LinkReader::new(&saved)
                .read(&saved.pages()[0])
                .len()
        );
        let before =
            crate::document::rasterize_untransformed(&pdf, &hayro::RenderCache::new(), 0, 600)
                .unwrap();
        let after =
            crate::document::rasterize_untransformed(&saved, &hayro::RenderCache::new(), 0, 600)
                .unwrap();
        assert_ne!(
            before.rgba, after.rgba,
            "highlight appearance must be rendered"
        );
        let mut edited = actual.clone();
        edited.contents = "修改后的 comment".into();
        save(&saved, &edited, &source).unwrap();
        let edited_pdf = crate::document::load(&source, "").unwrap();
        let reread = read(&edited_pdf, || false);
        assert_eq!(
            reread.items.len(),
            1,
            "editing must not duplicate annotations"
        );
        assert_eq!(reread.items[0].contents, edited.contents);
        assert_eq!(reread.items[0].rects, comment.rects);
    }

    #[test]
    fn cropped_rotated_pages_use_inverse_renderer_transform_for_new_highlights() {
        let root = tempfile::tempdir().unwrap();
        for rotation in [0, 90, 180, 270] {
            let mut document =
                lopdf::Document::load_mem(include_bytes!("../assets/pagewing-guide.pdf")).unwrap();
            let id = document.get_pages()[&1];
            let page = document.get_dictionary_mut(id).unwrap();
            page.set(
                "CropBox",
                vec![O::Integer(20), 30.into(), 550.into(), 730.into()],
            );
            page.set("Rotate", rotation);
            let source = root.path().join(format!("source-{rotation}.pdf"));
            document.save(&source).unwrap();
            let pdf = crate::document::load(&source, "").unwrap();
            let comment = draft();
            save(&pdf, &comment, &source).unwrap();
            let saved = crate::document::load(&source, "").unwrap();
            assert_eq!(
                read(&saved, || false).items[0].rects,
                comment.rects,
                "rotation {rotation}"
            );
        }
    }

    #[test]
    fn reads_direct_and_indirect_comments_after_bad_entries_and_edits_correct_slot() {
        let root = tempfile::tempdir().unwrap();
        for indirect in [false, true] {
            let mut doc =
                lopdf::Document::load_mem(include_bytes!("../assets/pagewing-guide.pdf")).unwrap();
            let page_id = doc.get_pages()[&1];
            let note = dictionary! { "Type" => "Annot", "Subtype" => "Text", "Contents" => lopdf::text_string("已有批注"), "T" => lopdf::text_string("作者"), "Rect" => vec![50.into(), 600.into(), 70.into(), 620.into()] };
            let highlight = dictionary! { "Type" => "Annot", "Subtype" => "Highlight", "Contents" => "Highlight comment", "Rect" => vec![80.into(), 600.into(), 180.into(), 620.into()], "RC" => "old rich text" };
            let reference = doc.add_object(note);
            let hidden = doc.add_object(dictionary! { "Subtype" => "Text", "Contents" => "hidden", "F" => 2, "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()] });
            let array = O::Array(vec![
                O::Null,
                O::Reference((99999, 0)),
                O::Reference(reference),
                O::Dictionary(highlight),
                O::Reference(hidden),
            ]);
            let annots = if indirect {
                O::Reference(doc.add_object(array))
            } else {
                array
            };
            doc.get_dictionary_mut(page_id)
                .unwrap()
                .set("Annots", annots);
            let source = root.path().join(format!("source-{indirect}.pdf"));
            doc.save(&source).unwrap();
            let pdf = crate::document::load(&source, "").unwrap();
            let comments = read(&pdf, || false);
            assert_eq!(comments.items.len(), 2);
            assert_eq!(comments.items[0].contents, "已有批注");
            assert_eq!(comments.items[0].author, "作者");
            assert_eq!(comments.items[0].slot, Some(2));
            assert_eq!(comments.items[1].slot, Some(3));
            for index in 0..2 {
                // Each branch starts from the same external-annotation fixture.
                std::fs::write(&source, pdf.data().as_ref()).unwrap();
                let mut edited = comments.items[index].clone();
                edited.contents = "edited 中文".into();
                save(&pdf, &edited, &source).unwrap();
                let saved = crate::document::load(&source, "").unwrap();
                let actual = read(&saved, || false);
                assert_eq!(actual.items[index].contents, edited.contents);
                assert_eq!(
                    actual.items[1 - index].contents,
                    comments.items[1 - index].contents
                );
            }
        }
    }

    #[test]
    fn stale_snapshots_readonly_files_and_invalid_edits_do_not_overwrite_the_pdf() {
        let root = tempfile::tempdir().unwrap();
        let (source, pdf) = source(root.path());
        let mut invalid = draft();
        invalid.page = 999;
        assert!(save(&pdf, &invalid, &source).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), pdf.data().as_ref());
        save(&pdf, &draft(), &source).unwrap();
        let saved = std::fs::read(&source).unwrap();
        assert!(
            save(&pdf, &draft(), &source)
                .unwrap_err()
                .contains("changed on disk")
        );
        assert_eq!(std::fs::read(&source).unwrap(), saved);
        let current = crate::document::load(&source, "").unwrap();
        let mut permissions = std::fs::metadata(&source).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&source, permissions).unwrap();
        assert!(
            save(&current, &draft(), &source)
                .unwrap_err()
                .contains("read-only")
        );
        assert_eq!(std::fs::read(&source).unwrap(), saved);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        assert!(read(&pdf, || true).items.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn saving_through_symlink_updates_target_and_preserves_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = tempfile::tempdir().unwrap();
        let (source, pdf) = source(root.path());
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o640)).unwrap();
        let alias = root.path().join("alias.pdf");
        symlink(&source, &alias).unwrap();
        save(&pdf, &draft(), &alias).unwrap();
        assert!(std::fs::symlink_metadata(&alias).unwrap().is_symlink());
        assert_eq!(
            std::fs::metadata(&source).unwrap().permissions().mode() & 0o777,
            0o640
        );
        let saved = crate::document::load(&source, "").unwrap();
        assert_eq!(read(&saved, || false).items[0].contents, draft().contents);
    }

    #[test]
    fn deleting_saved_highlight_restores_page_and_keeps_document_content() {
        let root = tempfile::tempdir().unwrap();
        let (source, original) = source(root.path());
        save(&original, &draft(), &source).unwrap();
        let highlighted = crate::document::load(&source, "").unwrap();
        let comment = read(&highlighted, || false).items.remove(0);
        delete(&highlighted, &comment, &source).unwrap();
        let deleted = crate::document::load(&source, "").unwrap();
        assert!(read(&deleted, || false).items.is_empty());
        let before =
            crate::document::rasterize_untransformed(&original, &hayro::RenderCache::new(), 0, 600)
                .unwrap();
        let after =
            crate::document::rasterize_untransformed(&deleted, &hayro::RenderCache::new(), 0, 600)
                .unwrap();
        assert_eq!(
            before.rgba, after.rgba,
            "deletion must remove the highlight appearance"
        );
        assert_eq!(
            crate::links::LinkReader::new(&original)
                .read(&original.pages()[0])
                .len(),
            crate::links::LinkReader::new(&deleted)
                .read(&deleted.pages()[0])
                .len()
        );
        let cache = hayro::hayro_interpret::InterpreterCache::new();
        assert_eq!(
            crate::text::extract(&original.pages()[0], &cache).text,
            crate::text::extract(&deleted.pages()[0], &cache).text
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        assert!(
            delete(&highlighted, &comment, &source)
                .unwrap_err()
                .contains("changed on disk")
        );
        assert!(
            delete(&deleted, &draft(), &source).is_err(),
            "unsaved drafts have no PDF annotation to delete"
        );
    }

    #[test]
    fn deletion_handles_raw_slots_indirect_arrays_popups_and_nested_replies() {
        for indirect_array in [false, true] {
            for indirect_comment in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let mut doc =
                    lopdf::Document::load_mem(include_bytes!("../assets/pagewing-guide.pdf"))
                        .unwrap();
                let page = doc.get_pages()[&1];
                let parent = doc.new_object_id();
                let popup = doc.add_object(dictionary! { "Subtype" => "Popup", "Parent" => parent, "Rect" => vec![0.into(), 0.into(), 20.into(), 20.into()] });
                let annotation = dictionary! { "Subtype" => "Highlight", "Contents" => lopdf::text_string("delete me"), "Popup" => popup, "Rect" => vec![50.into(), 600.into(), 150.into(), 620.into()] };
                doc.set_object(parent, annotation.clone());
                let unrelated = O::Dictionary(
                    dictionary! { "Subtype" => "Text", "Contents" => lopdf::text_string("keep me"), "Rect" => vec![200.into(), 600.into(), 220.into(), 620.into()] },
                );
                let mut entries = vec![O::Null, O::Reference((99999, 0)), O::Reference(popup)];
                if indirect_comment {
                    let reply = doc.add_object(dictionary! { "Subtype" => "Text", "IRT" => parent, "RT" => "R", "Contents" => lopdf::text_string("reply"), "Rect" => vec![30.into(), 30.into(), 40.into(), 40.into()] });
                    let nested = doc.add_object(dictionary! { "Subtype" => "Text", "IRT" => reply, "RT" => "R", "Contents" => lopdf::text_string("nested reply"), "Rect" => vec![30.into(), 30.into(), 40.into(), 40.into()] });
                    entries.extend([O::Reference(nested), O::Reference(reply)]);
                }
                let slot = entries.len();
                entries.push(if indirect_comment {
                    O::Reference(parent)
                } else {
                    O::Dictionary(annotation)
                });
                entries.push(unrelated);
                let expected = vec![
                    entries[0].clone(),
                    entries[1].clone(),
                    entries.last().unwrap().clone(),
                ];
                let array = O::Array(entries);
                let annots = if indirect_array {
                    O::Reference(doc.add_object(array))
                } else {
                    array
                };
                doc.get_dictionary_mut(page).unwrap().set("Annots", annots);
                let path = root.path().join("source.pdf");
                doc.save(&path).unwrap();
                let pdf = crate::document::load(&path, "").unwrap();
                let comment = read(&pdf, || false)
                    .items
                    .into_iter()
                    .find(|c| c.slot == Some(slot))
                    .unwrap();
                delete(&pdf, &comment, &path).unwrap();
                let saved = lopdf::Document::load(&path).unwrap();
                assert_eq!(
                    saved
                        .get_dictionary(page)
                        .unwrap()
                        .get(b"Annots")
                        .unwrap()
                        .as_array()
                        .unwrap(),
                    &expected
                );
                let pdf = crate::document::load(&path, "").unwrap();
                assert_eq!(
                    read(&pdf, || false)
                        .items
                        .iter()
                        .map(|c| c.contents.as_str())
                        .collect::<Vec<_>>(),
                    ["keep me"]
                );
            }
        }
    }

    #[test]
    fn deletion_supports_uneditable_contents_but_respects_locked_annotations() {
        for (kind, contents, flags) in [
            ("FreeText", "A free text note".into(), 0),
            ("Text", "x".repeat(65_537), 0),
            ("Highlight", "locked".into(), 128),
        ] {
            let root = tempfile::tempdir().unwrap();
            let mut doc =
                lopdf::Document::load_mem(include_bytes!("../assets/pagewing-guide.pdf")).unwrap();
            let page = doc.get_pages()[&1];
            let id = doc.add_object(dictionary! { "Subtype" => kind, "Contents" => lopdf::text_string(&contents), "F" => flags, "Rect" => vec![50.into(), 600.into(), 150.into(), 620.into()] });
            doc.get_dictionary_mut(page)
                .unwrap()
                .set("Annots", vec![O::Reference(id)]);
            let path = root.path().join("source.pdf");
            doc.save(&path).unwrap();
            let pdf = crate::document::load(&path, "").unwrap();
            let comment = read(&pdf, || false).items.remove(0);
            assert!(!comment.editable());
            if flags == 128 {
                assert!(delete(&pdf, &comment, &path).is_err());
                assert_eq!(std::fs::read(&path).unwrap(), pdf.data().as_ref());
            } else {
                delete(&pdf, &comment, &path).unwrap();
                assert!(
                    read(&crate::document::load(&path, "").unwrap(), || false)
                        .items
                        .is_empty()
                );
            }
        }
    }

    #[test]
    fn nested_replies_roundtrip_direct_parents_status_history_and_subtree_deletion() {
        for direct in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (path, pdf) = source(root.path());
            let baseline_doc = lopdf::Document::load(&path).unwrap();
            let baseline = page_annotations(&baseline_doc, baseline_doc.get_pages()[&1]).unwrap();
            save(&pdf, &draft(), &path).unwrap();
            if direct {
                let mut doc = lopdf::Document::load(&path).unwrap();
                let page = doc.get_pages()[&1];
                let mut entries = page_annotations(&doc, page).unwrap();
                let dict = doc
                    .get_dictionary(entries.last().unwrap().as_reference().unwrap())
                    .unwrap()
                    .clone();
                *entries.last_mut().unwrap() = O::Dictionary(dict);
                doc.get_dictionary_mut(page).unwrap().set("Annots", entries);
                doc.save(&path).unwrap();
            }
            for depth in 1..=3 {
                let pdf = crate::document::load(&path, "").unwrap();
                let comments = read(&pdf, || false);
                let mut reply = comments.items.last().unwrap().reply().unwrap();
                reply.contents = format!("第 {depth} 层回复");
                save(&pdf, &reply, &path).unwrap();
            }
            for status in ReviewStatus::ALL.into_iter().chain([ReviewStatus::None]) {
                let pdf = crate::document::load(&path, "").unwrap();
                let mut comment = read(&pdf, || false).items.remove(1);
                comment.review = status;
                comment.marked = !comment.marked;
                save(&pdf, &comment, &path).unwrap();
                let pdf = crate::document::load(&path, "").unwrap();
                let comments = read(&pdf, || false);
                assert_eq!(comments.items.len(), 4, "status records are not replies");
                assert_eq!(comments.items[1].review, status);
                assert_eq!(comments.items[1].marked, comment.marked);
                assert_eq!(comments.thread_rows(), [(0, 0), (1, 1), (2, 2), (3, 3)]);
                let doc = lopdf::Document::load(&path).unwrap();
                let page = doc.get_pages()[&1];
                let entries = page_annotations(&doc, page).unwrap();
                let state = doc
                    .get_dictionary(entries.last().unwrap().as_reference().unwrap())
                    .unwrap();
                assert!(matches!(state.get(b"StateModel").unwrap(), O::String(..)));
                assert!(matches!(state.get(b"State").unwrap(), O::String(..)));
                assert!(state.get(b"IRT").unwrap().as_reference().is_ok());
                assert_eq!(state.get(b"F").unwrap().as_i64().unwrap(), 2);
            }
            let pdf = crate::document::load(&path, "").unwrap();
            let comments = read(&pdf, || false);
            let mut edited = comments.items[2].clone();
            edited.contents = "Edited nested reply".into();
            save(&pdf, &edited, &path).unwrap();
            let pdf = crate::document::load(&path, "").unwrap();
            let mut sibling = read(&pdf, || false).items[0].reply().unwrap();
            sibling.contents = "Keep this sibling reply".into();
            save(&pdf, &sibling, &path).unwrap();
            let pdf = crate::document::load(&path, "").unwrap();
            let comments = read(&pdf, || false);
            assert_eq!(comments.items[2].parent, comments.items[1].id());
            assert_eq!(comments.items[2].contents, "Edited nested reply");
            delete(&pdf, &comments.items[1], &path).unwrap();
            let pdf = crate::document::load(&path, "").unwrap();
            let comments = read(&pdf, || false);
            assert_eq!(
                comments.items.len(),
                2,
                "deleting a reply keeps the root highlight and sibling branches"
            );
            assert_eq!(comments.items[0].contents, draft().contents);
            assert_eq!(comments.items[1].contents, "Keep this sibling reply");
            let doc = lopdf::Document::load(&path).unwrap();
            let remaining = page_annotations(&doc, doc.get_pages()[&1]).unwrap();
            assert_eq!(remaining.len(), baseline.len() + 2);
            assert_eq!(&remaining[..baseline.len()], baseline.as_slice());
        }
    }

    #[test]
    fn reads_acrobat_state_chains_out_of_order_and_deletes_across_pages() {
        let root = tempfile::tempdir().unwrap();
        let mut doc =
            lopdf::Document::load_mem(include_bytes!("../assets/pagewing-guide.pdf")).unwrap();
        let pages = doc.get_pages();
        let parent = doc.add_object(
            dictionary! { "Subtype" => "Highlight", "Contents" => lopdf::text_string("Root") },
        );
        let reply = doc.add_object(
            dictionary! { "Subtype" => "Text", "IRT" => parent, "Contents" => lopdf::text_string("Reply") },
        );
        let first = doc.add_object(dictionary! { "Subtype" => "Text", "IRT" => reply, "T" => lopdf::text_string("Acrobat reviewer"), "F" => 2, "StateModel" => lopdf::text_string("Review"), "State" => lopdf::text_string("Accepted") });
        let next = doc.add_object(dictionary! { "Subtype" => "Text", "IRT" => first, "T" => lopdf::text_string("Acrobat reviewer"), "F" => 2, "StateModel" => lopdf::text_string("Review"), "State" => lopdf::text_string("Completed") });
        let marked = doc.add_object(dictionary! { "Subtype" => "Text", "IRT" => parent, "StateModel" => lopdf::text_string("Marked"), "State" => lopdf::text_string("Marked") });
        let unrelated = doc.add_object(
            dictionary! { "Subtype" => "Text", "Contents" => lopdf::text_string("Keep me") },
        );
        let grouped = doc.add_object(dictionary! { "Subtype" => "Text", "IRT" => parent, "RT" => "Group", "Contents" => lopdf::text_string("Grouped markup") });
        let orphan = doc.add_object(
            dictionary! { "Subtype" => "Text", "IRT" => (99999, 0), "Contents" => lopdf::text_string("Orphan") },
        );
        let cycle = doc.new_object_id();
        doc.set_object(cycle, dictionary! { "Subtype" => "Text", "IRT" => cycle, "StateModel" => lopdf::text_string("Review"), "State" => lopdf::text_string("Rejected") });
        doc.get_dictionary_mut(pages[&1]).unwrap().set(
            "Annots",
            vec![
                O::Reference(next),
                first.into(),
                marked.into(),
                parent.into(),
                grouped.into(),
                cycle.into(),
            ],
        );
        doc.get_dictionary_mut(pages[&2]).unwrap().set(
            "Annots",
            vec![O::Reference(reply), unrelated.into(), orphan.into()],
        );
        let path = root.path().join("acrobat.pdf");
        doc.save(&path).unwrap();
        let pdf = crate::document::load(&path, "").unwrap();
        let comments = read(&pdf, || false);
        assert_eq!(comments.items.len(), 5);
        assert!(comments.items[0].marked);
        assert_eq!(comments.items[2].parent, comments.items[0].id());
        assert_eq!(comments.items[2].review, ReviewStatus::Completed);
        assert_eq!(comments.items[2].states.len(), 2);
        assert_eq!(comments.items[2].states[1].author, "Acrobat reviewer");
        assert!(
            comments.items[1].parent.is_none(),
            "RT Group is not a discussion reply"
        );
        assert!(
            comments.items[4].parent.is_none(),
            "missing IRT must not hide the annotation"
        );
        // A Pagewing change adds its own state without rewriting another author's record.
        let mut changed = comments.items[2].clone();
        changed.review = ReviewStatus::Rejected;
        save(&pdf, &changed, &path).unwrap();
        let pdf = crate::document::load(&path, "").unwrap();
        let comments = read(&pdf, || false);
        assert_eq!(comments.items[2].review, ReviewStatus::Rejected);
        assert_eq!(comments.items[2].states.len(), 3);
        delete(&pdf, &comments.items[2], &path).unwrap();
        let doc = lopdf::Document::load(&path).unwrap();
        assert_eq!(
            page_annotations(&doc, pages[&1]).unwrap(),
            [
                O::Reference(marked),
                parent.into(),
                grouped.into(),
                cycle.into()
            ]
        );
        assert_eq!(
            page_annotations(&doc, pages[&2]).unwrap(),
            [O::Reference(unrelated), orphan.into()]
        );
    }

    #[test]
    fn status_only_edits_preserve_truncated_contents_and_rich_text() {
        let root = tempfile::tempdir().unwrap();
        let mut doc =
            lopdf::Document::load_mem(include_bytes!("../assets/pagewing-guide.pdf")).unwrap();
        let page = doc.get_pages()[&1];
        let contents = "x".repeat(65_537);
        let id = doc.add_object(dictionary! { "Subtype" => "FreeText", "Contents" => lopdf::text_string(&contents), "RC" => lopdf::text_string("rich text") });
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Annots", vec![O::Reference(id)]);
        let path = root.path().join("long.pdf");
        doc.save(&path).unwrap();
        let pdf = crate::document::load(&path, "").unwrap();
        let mut comment = read(&pdf, || false).items.remove(0);
        assert!(comment.truncated && !comment.editable());
        comment.review = ReviewStatus::Completed;
        save(&pdf, &comment, &path).unwrap();
        let saved = lopdf::Document::load(&path).unwrap();
        assert_eq!(
            saved
                .get_dictionary(id)
                .unwrap()
                .get(b"Contents")
                .unwrap()
                .as_str()
                .unwrap(),
            contents.as_bytes()
        );
        assert_eq!(
            saved
                .get_dictionary(id)
                .unwrap()
                .get(b"RC")
                .unwrap()
                .as_str()
                .unwrap(),
            b"rich text"
        );
        let pdf = crate::document::load(&path, "").unwrap();
        assert_eq!(
            read(&pdf, || false).items[0].review,
            ReviewStatus::Completed
        );
    }

    #[test]
    fn thread_preorder_matches_reference_with_multiple_roots_and_malformed_relations() {
        fn visit(
            index: usize,
            depth: usize,
            parents: &[Option<usize>],
            rows: &mut Vec<(usize, usize)>,
            seen: &mut [bool],
        ) {
            if std::mem::replace(&mut seen[index], true) {
                return;
            }
            rows.push((index, depth));
            for (child, parent) in parents.iter().enumerate() {
                if *parent == Some(index) {
                    visit(child, depth + 1, parents, rows, seen);
                }
            }
        }
        for mut seed in 0..64_u64 {
            for count in [0, 1, 2, 7, 24, 96] {
                let mut comments = Comments::default();
                for index in 0..count {
                    let mut comment = draft();
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    comment.page = index / 12;
                    // Include missing and duplicate IDs as well as valid IDs.
                    comment.slot = (seed % 7 != 0).then_some(index % 9);
                    comment.parent = match seed % 5 {
                        0 => None,
                        1 => comment.id(),
                        _ => Some(CommentId {
                            page: (seed >> 32) as usize % (count / 12 + 2),
                            slot: (seed >> 48) as usize % 12,
                        }),
                    };
                    comments.items.push(comment);
                }
                // Resolve IDs independently with a reverse linear search, then
                // recursively walk children in source order for this small graph.
                let parents: Vec<_> = comments
                    .items
                    .iter()
                    .enumerate()
                    .map(|(index, comment)| {
                        comment
                            .parent
                            .and_then(|id| comments.items.iter().rposition(|c| c.id() == Some(id)))
                            .filter(|&parent| parent != index)
                    })
                    .collect();
                let mut expected = Vec::new();
                let mut seen = vec![false; count];
                for root in (0..count).filter(|&i| parents[i].is_none()).chain(0..count) {
                    visit(root, 0, &parents, &mut expected, &mut seen);
                }
                assert_eq!(
                    comments.thread_rows(),
                    expected,
                    "{count} comments, seed {seed}"
                );
                assert_eq!(expected.len(), count);
            }
        }
    }

    #[test]
    fn wide_threads_keep_source_order_with_parents_after_their_replies() {
        let count = 10_000;
        let roots = [count / 2, count - 1];
        let comments = Comments {
            items: (0..count)
                .map(|index| {
                    let mut comment = draft();
                    comment.slot = Some(index);
                    comment.parent = (!roots.contains(&index)).then_some(CommentId {
                        page: 0,
                        slot: roots[index % 2],
                    });
                    comment
                })
                .collect(),
            limited: false,
        };
        let mut expected = Vec::with_capacity(count);
        for (group, root) in roots.into_iter().enumerate() {
            expected.push((root, 0));
            expected.extend(
                (0..count)
                    .filter(|index| !roots.contains(index) && index % 2 == group)
                    .map(|index| (index, 1)),
            );
        }
        assert_eq!(comments.thread_rows(), expected);
    }

    #[test]
    fn malformed_cycles_and_deep_threads_show_each_comment_once() {
        let mut comments = Comments::default();
        for index in 0..10_000 {
            let mut comment = draft();
            comment.slot = Some(index);
            comment.parent = Some(CommentId {
                page: 0,
                slot: if index == 0 { 9999 } else { index - 1 },
            });
            comments.items.push(comment);
        }
        let rows = comments.thread_rows();
        assert_eq!(rows.len(), 10_000);
        assert_eq!(rows[0], (0, 0));
        assert_eq!(rows[9999], (9999, 9999));
    }
}
