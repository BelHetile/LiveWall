use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSNumber, NSString};

pub struct Display {
    pub uuid: String,
    pub id: u32,
    pub frame: CGRect,
    pub scale: f64,
}

impl Display {
    pub fn main(mtm: MainThreadMarker) -> Self {
        let screens = NSScreen::screens(mtm);
        if screens.count() == 0 {
            return Self { uuid: String::new(), id: 0, frame: CGRect::ZERO, scale: 1.0 };
        }
        let screen = screens.objectAtIndex(0);
        return Self {
            uuid: uuid(&screen),
            id: screen_number(&screen).unwrap_or(0),
            frame: screen.frame(),
            scale: screen.backingScaleFactor(),
        };
    }

    pub fn max_px(&self) -> u32 {
        let width = pixel_size(self.frame.size.width * self.scale);
        let height = pixel_size(self.frame.size.height * self.scale);
        return width.max(height);
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn pixel_size(points: f64) -> u32 {
    if !points.is_finite() || points <= 0.0 {
        return 0;
    }
    return points.min(f64::from(u32::MAX)) as u32;
}

fn screen_number(screen: &NSScreen) -> Option<u32> {
    let description = screen.deviceDescription();
    return description
        .objectForKey(&NSString::from_str("NSScreenNumber"))
        .and_then(|value| value.downcast_ref::<NSNumber>().map(NSNumber::unsignedIntValue));
}

fn uuid(screen: &NSScreen) -> String {
    if let Some(identifier) = screen_number(screen) {
        let uuid = unsafe { objc2_color_sync::CGDisplayCreateUUIDFromDisplayID(identifier) };
        if let Some(string) = objc2_core_foundation::CFUUID::new_string(None, Some(&uuid)) {
            return string.to_string();
        }
    }
    return screen.localizedName().to_string();
}
