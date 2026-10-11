//! Texture and RSI caches. Images are read straight from the upstream
//! resource roots through `vellum` (not Bevy's asset server, whose paths are
//! relative to a single `assets/` dir) and uploaded once per path.

use std::collections::HashMap;
use std::sync::Arc;

use athanor::Content;
use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use vellum::rsi::Rsi;
use vellum::ResPath;

#[derive(Clone)]
pub struct Tex {
    pub handle: Handle<Image>,
    pub size: UVec2,
}

#[derive(Resource, Default)]
pub struct TextureCache {
    images: HashMap<ResPath, Option<Tex>>,
    rsis: HashMap<ResPath, Option<Arc<Rsi>>>,
}

impl TextureCache {
    /// Load (once) a PNG by resource path.
    pub fn image(&mut self, content: &Content, images: &mut Assets<Image>, path: &ResPath) -> Option<Tex> {
        if let Some(t) = self.images.get(path) {
            return t.clone();
        }
        let tex = content.res.read(path).ok().and_then(|bytes| {
            match Image::from_buffer(
                &bytes,
                ImageType::Extension("png"),
                CompressedImageFormats::NONE,
                true,
                ImageSampler::nearest(),
                RenderAssetUsages::default(),
            ) {
                Ok(img) => {
                    let size = img.size();
                    Some(Tex {
                        handle: images.add(img),
                        size,
                    })
                }
                Err(e) => {
                    warn!("bad image {path}: {e}");
                    None
                }
            }
        });
        self.images.insert(path.clone(), tex.clone());
        tex
    }

    /// Load (once) an RSI's metadata. `rsi` is relative to `/Textures/`.
    pub fn rsi(&mut self, content: &Content, rsi: &str) -> Option<Arc<Rsi>> {
        let path = ResPath::texture(rsi);
        if let Some(r) = self.rsis.get(&path) {
            return r.clone();
        }
        let r = Rsi::load(&content.res, &path).ok().map(Arc::new);
        if r.is_none() {
            debug!("missing RSI {path}");
        }
        self.rsis.insert(path, r.clone());
        r
    }

    /// The image for one RSI state.
    pub fn state_image(&mut self, content: &Content, images: &mut Assets<Image>, rsi: &Rsi, state: &str) -> Option<Tex> {
        rsi.state(state)?;
        self.image(content, images, &rsi.state_png(state))
    }
}
