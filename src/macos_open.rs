//! Finder's Open Documents Apple event is separate from argv and file drops.

use std::{cell::RefCell, path::PathBuf};

use eframe::egui;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, sel};
use objc2_app_kit::{NSApplication, NSApplicationWillFinishLaunchingNotification};
use objc2_foundation::{
    MainThreadMarker, NSAppleEventDescriptor, NSAppleEventManager, NSNotification,
    NSNotificationCenter, NSObject, NSObjectProtocol,
};

const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
const OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
const ERROR_NUMBER: u32 = u32::from_be_bytes(*b"errn");

#[derive(Default)]
struct Pending {
    paths: Vec<PathBuf>,
    context: Option<egui::Context>,
}

impl Pending {
    fn enqueue(&mut self, paths: Vec<PathBuf>) {
        self.paths.extend(paths);
        if let Some(context) = &self.context {
            context.request_repaint();
        }
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements. All access stays on the
    // main thread, and the handler lives until it is unregistered in Drop.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = RefCell<Pending>]
    struct OpenDocumentsHandler;

    unsafe impl NSObjectProtocol for OpenDocumentsHandler {}

    impl OpenDocumentsHandler {
        #[unsafe(method(willFinishLaunching:))]
        fn will_finish_launching(&self, _notification: &NSNotification) {
            self.register();
        }

        #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
        fn open_documents(&self, event: &NSAppleEventDescriptor, reply: &NSAppleEventDescriptor) {
            match document_paths(event) {
                Some(paths) => self.ivars().borrow_mut().enqueue(paths),
                None => reply.setParamDescriptor_forKeyword(
                    &NSAppleEventDescriptor::descriptorWithInt32(-1700), // errAECoercionFail
                    ERROR_NUMBER,
                ),
            }
        }
    }
);

impl OpenDocumentsHandler {
    fn register(&self) {
        // SAFETY: The selector has the two-descriptor signature required by
        // NSAppleEventManager, and the owner retains us until unregistration.
        unsafe {
            NSAppleEventManager::sharedAppleEventManager()
                .setEventHandler_andSelector_forEventClass_andEventID(
                    self,
                    sel!(handleOpenDocuments:withReplyEvent:),
                    CORE_EVENT_CLASS,
                    OPEN_DOCUMENTS,
                );
        }
    }
}

fn document_paths(event: &NSAppleEventDescriptor) -> Option<Vec<PathBuf>> {
    let list = event.paramDescriptorForKeyword(DIRECT_OBJECT)?;
    if list.numberOfItems() == 0 {
        return None;
    }
    // Use NSURL's filesystem conversion, preserving spaces, Unicode and
    // percent escapes rather than treating a URL string as a filesystem path.
    (1..=list.numberOfItems())
        .map(|index| {
            list.descriptorAtIndex(index)?
                .fileURLValue()?
                .to_file_path()
        })
        .collect()
}

pub struct OpenDocuments {
    handler: Retained<OpenDocumentsHandler>,
}

impl OpenDocuments {
    pub fn install() -> Self {
        let mtm = MainThreadMarker::new().expect("the reader starts on the main thread");
        let application = NSApplication::sharedApplication(mtm);
        let handler = OpenDocumentsHandler::alloc(mtm).set_ivars(RefCell::default());
        // SAFETY: NSObject's init has the exact Objective-C signature.
        let handler: Retained<OpenDocumentsHandler> = unsafe { msg_send![super(handler), init] };
        // AppKit installs its default Apple event handlers during launch. Replace
        // Open Documents in willFinishLaunching, before cold-start files arrive
        // (and before eframe creates the reader in didFinishLaunching).
        // https://developer.apple.com/documentation/appkit/nsapplication
        // SAFETY: The selector takes one NSNotification, and the observer is
        // retained by this owner until it is removed in Drop.
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                &handler,
                sel!(willFinishLaunching:),
                Some(NSApplicationWillFinishLaunchingNotification),
                Some(&application),
            );
        }
        Self { handler }
    }

    pub fn attach(&self, context: &egui::Context) {
        // Also support attachment to an application that has already launched.
        self.handler.register();
        self.handler.ivars().borrow_mut().context = Some(context.clone());
        context.request_repaint();
    }

    pub fn take_paths(&self) -> Vec<PathBuf> {
        std::mem::take(&mut self.handler.ivars().borrow_mut().paths)
    }
}

impl Drop for OpenDocuments {
    fn drop(&mut self) {
        // SAFETY: This is the same observer registered by install, still alive.
        unsafe { NSNotificationCenter::defaultCenter().removeObserver(&self.handler) };
        NSAppleEventManager::sharedAppleEventManager()
            .removeEventHandlerForEventClass_andEventID(CORE_EVENT_CLASS, OPEN_DOCUMENTS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::NSURL;

    fn empty_event() -> Retained<NSAppleEventDescriptor> {
        NSAppleEventDescriptor::appleEventWithEventClass_eventID_targetDescriptor_returnID_transactionID(
            CORE_EVENT_CLASS, OPEN_DOCUMENTS, None, -1, 0,
        )
    }

    fn event(paths: &[PathBuf]) -> Retained<NSAppleEventDescriptor> {
        let event = empty_event();
        let list = NSAppleEventDescriptor::listDescriptor();
        for (index, path) in paths.iter().enumerate() {
            let url = NSURL::from_file_path(path).unwrap();
            let descriptor = NSAppleEventDescriptor::descriptorWithFileURL(&url);
            list.insertDescriptor_atIndex(&descriptor, (index + 1) as isize);
        }
        event.setParamDescriptor_forKeyword(&list, DIRECT_OBJECT);
        event
    }

    #[test]
    fn decodes_multiple_file_urls_with_unicode_and_escaped_characters() {
        let paths = vec![
            PathBuf::from("/tmp/中文 paper #1 100%.pdf"),
            PathBuf::from("/tmp/another paper.pdf"),
        ];
        assert_eq!(document_paths(&event(&paths)), Some(paths));
    }

    #[test]
    fn rejects_missing_empty_or_non_file_arguments() {
        assert!(document_paths(&NSAppleEventDescriptor::new()).is_none());
        assert!(document_paths(&event(&[])).is_none());
        let event = empty_event();
        let list = NSAppleEventDescriptor::listDescriptor();
        list.insertDescriptor_atIndex(&NSAppleEventDescriptor::nullDescriptor(), 1);
        event.setParamDescriptor_forKeyword(&list, DIRECT_OBJECT);
        assert!(document_paths(&event).is_none());
    }
}
