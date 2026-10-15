use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::runtime::Handle;

pub const EVICT_AFTER: Duration = Duration::from_mins(10);

pub fn evict_after<K, V, F>(handle: &Handle, shared: Arc<Mutex<BTreeMap<K, V>>>, key: K, removable: F)
where
    K: Ord + Send + 'static,
    V: Send + 'static,
    F: Fn(&V) -> bool + Send + 'static,
{
    handle.spawn(async move {
        tokio::time::sleep(EVICT_AFTER).await;
        if let Ok(mut map) = shared.lock()
            && map.get(&key).is_none_or(&removable)
        {
            map.remove(&key);
        }
    });
}
