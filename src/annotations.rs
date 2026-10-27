//! Standard PDF markup annotations. Geometry is kept in renderer coordinates.
use hayro::{
    hayro_interpret::util::TransformExt,
    hayro_syntax::{
        Pdf,
        object::{Array, Dict, MaybeRef, Name, Object, Rect as PdfRect, String as PdfString},
    },
    kurbo::{Point, Rect},
};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Comment {
    pub page: usize,
    /// Raw /Annots slot; None creates a new highlight.
    pub slot: Option<usize>,
    pub kind: String,
    pub contents: String,
    pub author: String,
    pub rects: Vec<Rect>,
    pub has_appearance: bool,
    pub truncated: bool,
    pub locked: bool,
}

impl Comment {
    pub fn editable(&self) -> bool {
        !self.locked && !self.truncated && self.kind != "FreeText"
    }
}

#[derive(Default)]
pub struct Comments {
    pub items: Vec<Comment>,
    pub limited: bool,
}

pub fn read(pdf: &Pdf, cancelled: impl Fn() -> bool) -> Comments {
    let mut result = Comments::default();
    let mut budget = 100_000usize;
    for (page_index, page) in pdf.pages().iter().enumerate() {
        if cancelled() {
            return result;
        }
        let Some(array) = page.raw().get::<Array<'_>>(b"Annots") else {
            continue;
        };
        let transform = page.initial_transform(true).to_kurbo();
        for (slot, object) in array.raw_iter().enumerate() {
            if budget == 0 || result.items.len() >= 10_000 {
                result.limited = true;
                return result;
            }
            budget -= 1;
            if cancelled() {
                return result;
            }
            let dict = match object {
                MaybeRef::Ref(id) => pdf.xref().get::<Dict<'_>>(id.into()),
                MaybeRef::NotRef(Object::Dict(dict)) => Some(dict),
                _ => None,
            };
            let Some(dict) = dict else { continue };
            let Some(kind) = dict.get::<Name<'_>>(b"Subtype") else {
                continue;
            };
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
            let author = dict
                .get::<PdfString<'_>>(b"T")
                .map(|s| crate::pdf_string::decode_prefix(s.as_bytes(), 1024).0)
                .unwrap_or_default();
            let mut rects = Vec::new();
            if let Some(quads) = dict.get::<Array<'_>>(b"QuadPoints") {
                let numbers: Vec<_> = quads.iter::<f32>().take(8192).collect();
                for quad in numbers.as_chunks::<8>().0 {
                    let points: Vec<_> = quad
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|p| transform * Point::new(p[0] as f64, p[1] as f64))
                        .collect();
                    let rect = points.iter().fold(
                        Rect::new(
                            f64::INFINITY,
                            f64::INFINITY,
                            f64::NEG_INFINITY,
                            f64::NEG_INFINITY,
                        ),
                        |r, p| {
                            Rect::new(r.x0.min(p.x), r.y0.min(p.y), r.x1.max(p.x), r.y1.max(p.y))
                        },
                    );
                    if rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0 {
                        rects.push(rect);
                    }
                }
            }
            if rects.is_empty()
                && let Some(r) = dict.get::<PdfRect>(b"Rect")
            {
                let r = transform.transform_rect_bbox(Rect::new(r.x0, r.y0, r.x1, r.y1));
                if r.is_finite() && r.width() > 0.0 && r.height() > 0.0 {
                    rects.push(r);
                }
            }
            result.items.push(Comment {
                page: page_index,
                slot: Some(slot),
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
    result
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

/// Remove the selected annotation, its popup and replies from the page.
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
    if comment.locked || (operation == Operation::Save && !comment.editable()) {
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
        let slot = comment
            .slot
            .ok_or("This comment has not been saved. Use Discard to remove the draft.")?;
        annots = without_comment(previous, &annots, slot)?;
    } else if let Some(slot) = comment.slot {
        let value = annots.get(slot).ok_or("Annotation no longer exists")?;
        let mut dict = previous
            .dereference(value)
            .and_then(|(_, v)| v.as_dict())
            .map_err(|e| e.to_string())?
            .clone();
        dict.set("Contents", lopdf::text_string(&comment.contents));
        dict.remove(b"RC"); // Plain text is now authoritative.
        let id = if let O::Reference(id) = value {
            *id
        } else {
            update.new_document.new_object_id()
        };
        update.new_document.set_object(id, dict);
        annots[slot] = O::Reference(id);
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
            "Contents" => lopdf::text_string(&comment.contents), "T" => lopdf::text_string("Folio"),
            "C" => array(vec![1.0, 0.85, 0.0]), "CA" => 0.35f32, "F" => 4,
            "AP" => dictionary! { "N" => appearance_id }, "P" => page_id,
        });
        annots.push(O::Reference(id));
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

/// Preserve raw /Annots entries, including unrelated and malformed objects.
/// A work queue follows popup/reply links without repeatedly scanning the array.
fn without_comment(
    document: &lopdf::Document,
    annots: &[lopdf::Object],
    slot: usize,
) -> Result<Vec<lopdf::Object>, String> {
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
    Ok(annots
        .iter()
        .enumerate()
        .filter(|(index, _)| !removed[*index])
        .map(|(_, value)| value.clone())
        .collect())
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
            kind: "Highlight".into(),
            contents: "中文批注 📝\nSecond line".into(),
            author: "Folio".into(),
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
        std::fs::write(&path, include_bytes!("../assets/folio-guide.pdf")).unwrap();
        let pdf = crate::document::load(&path, "").unwrap();
        (path, pdf)
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
                lopdf::Document::load_mem(include_bytes!("../assets/folio-guide.pdf")).unwrap();
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
                lopdf::Document::load_mem(include_bytes!("../assets/folio-guide.pdf")).unwrap();
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
                    lopdf::Document::load_mem(include_bytes!("../assets/folio-guide.pdf")).unwrap();
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
                lopdf::Document::load_mem(include_bytes!("../assets/folio-guide.pdf")).unwrap();
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
}
