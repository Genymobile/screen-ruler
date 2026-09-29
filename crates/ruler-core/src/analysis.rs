//! Everything derived from a monitor's screenshot before the user interacts:
//! its edge map.
//!
//! Canny is the expensive step (tens of milliseconds per monitor in a
//! release build), so monitors are analysed in parallel. Running it off the
//! UI thread is the caller's business.

use std::borrow::Borrow;

use image::RgbaImage;

use crate::edges::{self, EdgeMap};

/// Edge map of one screenshot with the given Canny thresholds.
pub fn analyse(image: &RgbaImage, (low, high): (u16, u16)) -> EdgeMap {
    edges::canny(&edges::to_gray(image), low, high)
}

/// Analyses every image in parallel, one thread each, in input order.
pub fn analyse_all<I>(images: &[I], thresholds: (u16, u16)) -> Vec<EdgeMap>
where
    I: Borrow<RgbaImage> + Sync,
{
    std::thread::scope(|scope| {
        let workers: Vec<_> = images
            .iter()
            .map(|image| scope.spawn(move || analyse(image.borrow(), thresholds)))
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().expect("edge analysis panicked"))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn every_monitor_gets_an_analysis_in_order() {
        // A flat image has no edges; one with a hard step has some.
        let flat = RgbaImage::from_pixel(32, 32, Rgba([128, 128, 128, 255]));
        let step = RgbaImage::from_fn(32, 32, |x, _| {
            let v = if x < 16 { 0 } else { 255 };
            Rgba([v, v, v, 255])
        });

        let results = analyse_all(&[flat, step], edges::sensitivity_to_thresholds(85.0));
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].count(), 0);
        assert!(results[1].count() > 0);
    }
}
