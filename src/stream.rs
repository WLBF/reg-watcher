use super::*;
use failure::Error;
use futures::stream::Stream;
use futures::prelude::*;

impl Stream for Watcher {
    type Item = WatchResponse;
    type Error = Error;

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        if let Some(ref key) = self.reg_key {
            let notify_filter = self.notify_filter;
            let watch_subtree = self.watch_subtree;
            watch(key, notify_filter, watch_subtree, Timeout::Infinite)
                .map(|v| Async::Ready(Some(v)))
        } else {
            Err(format_err!("watcher none registry handle"))
        }
    }
}
