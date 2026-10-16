use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_foundation::{NSError, NSString};
use objc2_local_authentication::{LAContext, LAPolicy};

use crate::app::{Event, post};

pub fn evaluate(reason: &str, epoch: u64) -> Retained<LAContext> {
    let context = unsafe { LAContext::new() };
    let reply = RcBlock::new(move |success: Bool, _error: *mut NSError| {
        post(Event::UnlockResult { success: success.as_bool(), epoch });
    });
    let reason = NSString::from_str(reason);
    unsafe {
        context.evaluatePolicy_localizedReason_reply(LAPolicy::DeviceOwnerAuthentication, &reason, &reply);
    }
    return context;
}

pub fn cancel(context: &LAContext) {
    unsafe { context.invalidate() };
}
