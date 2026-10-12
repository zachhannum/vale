//! The file services of iOS for the projects folders: iCloud Drive, file
//! coordination, the versions of a file, and the files that the Files app
//! gives to the app.

// The app has no scene manifest, so the system uses the app delegate.
#![allow(deprecated)]

use std::cell::{Cell, RefCell};
use std::io;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::time::{Duration, SystemTime};

use block2::{RcBlock, StackBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{
    NSDictionary, NSError, NSFileCoordinator, NSFileCoordinatorReadingOptions,
    NSFileCoordinatorWritingOptions, NSFileManager, NSFileVersion, NSFileVersionReplacingOptions,
    NSNotification, NSNotificationCenter, NSNotificationName, NSNumber, NSString, NSURL,
    NSURLUbiquitousItemDownloadingStatusKey, NSURLUbiquitousItemDownloadingStatusNotDownloaded,
    NSURLUbiquitousItemIsDownloadingKey,
};
use objc2_ui_kit::{
    UIApplication, UIApplicationDelegate, UIApplicationDidBecomeActiveNotification,
    UIApplicationDidFinishLaunchingNotification, UIApplicationLaunchOptionsURLKey,
    UIApplicationOpenURLOptionsKey,
};

use super::{Availability, Files, SystemEvents, Version};

fn url_of(path: &Path) -> io::Result<Retained<NSURL>> {
    NSURL::from_file_path(path).ok_or_else(|| io::ErrorKind::InvalidInput.into())
}

fn io_error(error: Retained<NSError>) -> io::Error {
    io::Error::other(error.localizedDescription().to_string())
}

/// One resource value of a file. `None`: the file does not have the value.
fn resource(url: &NSURL, key: &NSString) -> Option<Retained<AnyObject>> {
    let mut value = None;
    // SAFETY: `key` is a resource key of the system, and `value` gets an
    // object or stays empty.
    unsafe { url.getResourceValue_forKey_error(&mut value, key) }.ok()?;
    value
}

/// Runs `f` in a coordinated read or write of a file.
fn coordinate(
    path: &Path,
    write: bool,
    f: &mut dyn FnMut(&Path) -> io::Result<()>,
) -> io::Result<()> {
    let url = url_of(path)?;
    let f = RefCell::new(f);
    let result: Cell<Option<io::Result<()>>> = Cell::new(None);
    let (f, result) = (&f, &result);
    let block = StackBlock::new(move |url: NonNull<NSURL>| {
        // SAFETY: the coordinator gives a live URL for the time of the call.
        let path = unsafe { url.as_ref() }.to_file_path();
        let done = match (path, f.try_borrow_mut()) {
            (Some(path), Ok(mut f)) => (*f)(&path),
            _ => Err(io::ErrorKind::InvalidInput.into()),
        };
        result.set(Some(done));
    });
    let coordinator = NSFileCoordinator::new();
    let mut error = None;
    if write {
        coordinator.coordinateWritingItemAtURL_options_error_byAccessor(
            &url,
            NSFileCoordinatorWritingOptions(0),
            Some(&mut error),
            &block,
        );
    } else {
        coordinator.coordinateReadingItemAtURL_options_error_byAccessor(
            &url,
            NSFileCoordinatorReadingOptions(0),
            Some(&mut error),
            &block,
        );
    }
    match (result.take(), error) {
        (Some(done), _) => done,
        (None, Some(error)) => Err(io_error(error)),
        (None, None) => Err(io::Error::other("The system did not give the file.")),
    }
}

/// The versions of a file that wait for a choice.
fn conflict_versions(url: &NSURL) -> Vec<Retained<NSFileVersion>> {
    let versions = NSFileVersion::unresolvedConflictVersionsOfItemAtURL(url);
    versions.map(|list| list.to_vec()).unwrap_or_default()
}

/// The file services of iOS.
pub struct CloudFiles;

impl Files for CloudFiles {
    fn availability(&self, path: &Path) -> Availability {
        let Ok(url) = url_of(path) else {
            return Availability::Here;
        };
        // SAFETY: the statics are constants of Foundation.
        let (status_key, downloading_key, not_downloaded) = unsafe {
            (
                NSURLUbiquitousItemDownloadingStatusKey,
                NSURLUbiquitousItemIsDownloadingKey,
                NSURLUbiquitousItemDownloadingStatusNotDownloaded,
            )
        };
        let status = resource(&url, status_key).and_then(|v| v.downcast::<NSString>().ok());
        if status.is_none_or(|status| &*status != not_downloaded) {
            return Availability::Here;
        }
        let downloading =
            resource(&url, downloading_key).and_then(|v| v.downcast::<NSNumber>().ok());
        if downloading.is_some_and(|number| number.as_bool()) {
            Availability::Downloading
        } else {
            Availability::InCloud
        }
    }

    fn start_download(&self, path: &Path) -> io::Result<()> {
        let url = url_of(path)?;
        let manager = NSFileManager::defaultManager();
        manager
            .startDownloadingUbiquitousItemAtURL_error(&url)
            .map_err(io_error)
    }

    fn conflicts(&self, path: &Path) -> Vec<Version> {
        let Ok(url) = url_of(path) else {
            return Vec::new();
        };
        let version = |version: Retained<NSFileVersion>| {
            let device = version.localizedNameOfSavingComputer();
            let since = version
                .modificationDate()
                .map(|d| d.timeIntervalSince1970());
            let since = since.and_then(|secs| Duration::try_from_secs_f64(secs).ok());
            Version {
                device: device.map_or_else(|| "another device".to_string(), |d| d.to_string()),
                modified: since.map(|since| SystemTime::UNIX_EPOCH + since),
            }
        };
        conflict_versions(&url).into_iter().map(version).collect()
    }

    fn resolve(&self, path: &Path, keep: Option<usize>) -> io::Result<()> {
        coordinate(path, true, &mut |path| {
            let url = url_of(path)?;
            let versions = conflict_versions(&url);
            if let Some(index) = keep {
                let version = versions.get(index).ok_or(io::ErrorKind::NotFound)?;
                version
                    .replaceItemAtURL_options_error(&url, NSFileVersionReplacingOptions(0))
                    .map_err(io_error)?;
            }
            for version in &versions {
                version.setResolved(true);
            }
            NSFileVersion::removeOtherVersionsOfItemAtURL_error(&url).map_err(io_error)
        })
    }

    fn read(&self, path: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
        coordinate(path, false, f)
    }

    fn write(&self, path: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
        coordinate(path, true, f)
    }

    fn with_access(&self, path: &Path, f: &mut dyn FnMut() -> io::Result<()>) -> io::Result<()> {
        let url = url_of(path)?;
        // SAFETY: each start that returns `true` has one stop.
        let scoped = unsafe { url.startAccessingSecurityScopedResource() };
        let result = f();
        if scoped {
            // SAFETY: see above.
            unsafe { url.stopAccessingSecurityScopedResource() };
        }
        result
    }
}

/// The Documents folder of the iCloud container. `None`: the device has no
/// iCloud account, or the build has no iCloud entitlement. The call can
/// take a long time, so it must not run on the main thread.
fn container_documents() -> Option<PathBuf> {
    let manager = NSFileManager::defaultManager();
    manager.ubiquityIdentityToken()?;
    let container = manager.URLForUbiquityContainerIdentifier(None)?;
    let dir = container.to_file_path()?.join("Documents");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

struct Ivars {
    system: SystemEvents,
    /// The file of the launch, which the system can give two times.
    launch: RefCell<Option<PathBuf>>,
}

define_class!(
    // SAFETY: NSObject allows subclasses, and the type has no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ValeAppDelegate"]
    #[ivars = Ivars]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl UIApplicationDelegate for AppDelegate {
        #[unsafe(method(application:openURL:options:))]
        fn open_url(
            &self,
            _app: &UIApplication,
            url: &NSURL,
            _options: &NSDictionary<UIApplicationOpenURLOptionsKey, AnyObject>,
        ) -> bool {
            let path = url.to_file_path();
            if let Some(path) = &path {
                let ivars = self.ivars();
                if ivars.launch.take().as_ref() != Some(path) {
                    ivars.system.open(path.clone());
                }
            }
            path.is_some()
        }
    }
);

/// Observes a notification of the app for the life of the process.
fn observe(name: &NSNotificationName, f: impl Fn(&NSNotification) + 'static) {
    let block = RcBlock::new(move |notification: NonNull<NSNotification>| {
        // SAFETY: the center gives a live notification for the time of the
        // call.
        f(unsafe { notification.as_ref() });
    });
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: the block is `'static`, and it runs on the thread that posts
    // the notification. UIKit posts these notifications on the main thread.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
    };
    std::mem::forget(token);
}

/// Connects the app to the system. Call it on the main thread before the
/// window starts, because the system gives the file of the launch while the
/// app starts.
pub fn install(system: SystemEvents) {
    // SAFETY: the statics are constants of UIKit.
    let (launched, active) = unsafe {
        (
            UIApplicationDidFinishLaunchingNotification,
            UIApplicationDidBecomeActiveNotification,
        )
    };

    // winit starts the app with no delegate. This delegate gets the files
    // that the Files app opens in the app.
    let events = system.clone();
    observe(launched, move |notification| {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        // SAFETY: the static is a constant of UIKit.
        let key: &NSString = unsafe { UIApplicationLaunchOptionsURLKey };
        let info = notification.userInfo();
        let url = info.as_ref().and_then(|info| {
            // SAFETY: the keys of the launch options are strings.
            let info = unsafe { info.cast_unchecked::<NSString, AnyObject>() };
            info.objectForKey(key)
        });
        let launch = url
            .and_then(|url| url.downcast::<NSURL>().ok())
            .and_then(|url| url.to_file_path());
        if let Some(path) = &launch {
            events.open(path.clone());
        }
        let this = AppDelegate::alloc(mtm).set_ivars(Ivars {
            system: events.clone(),
            launch: RefCell::new(launch),
        });
        // SAFETY: NSObject has the initializer `init`.
        let delegate: Retained<AppDelegate> = unsafe { msg_send![super(this), init] };
        let app = UIApplication::sharedApplication(mtm);
        // SAFETY: the app does not retain its delegate, so the delegate
        // lives for the life of the process.
        unsafe { app.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
        std::mem::forget(delegate);
    });

    // The account of the device can change while the app is in the
    // background.
    observe(active, move |_| {
        let events = system.clone();
        std::thread::spawn(move || events.set_cloud(container_documents()));
    });
}
