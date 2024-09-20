use galileo::galileo_types::geo::NewGeoPoint;
use std::sync::{Arc, RwLock};

pub struct Map {
    event_processor: galileo::control::EventProcessor,
    renderer: Arc<RwLock<galileo::render::WgpuRenderer>>,
    map: Arc<RwLock<galileo::Map>>,
}

impl Map {
    pub fn new(
        window: Arc<winit::window::Window>,
        device: Arc<wgpu::Device>,
        surface: Arc<wgpu::Surface<'static>>,
        queue: Arc<wgpu::Queue>,
        config: wgpu::SurfaceConfiguration,
    ) -> Self {
        let renderer = galileo::render::WgpuRenderer::new_with_device_and_surface(
            device, surface, queue, config,
        );
        let renderer = Arc::new(RwLock::new(renderer));
        let mut event_processor = galileo::control::EventProcessor::default();
        event_processor.add_handler(galileo::control::MapController::default());

        let builder = galileo::MapBuilder::new();
        builder
            .center(galileo::galileo_types::geo::impls::GeoPoint2d::latlon(
                42.4435, -123.3260,
            ))
            .resolution(galileo::TileSchema::web(18).lod_resolution(12).unwrap())
            .with_raster_tiles(
                |index| {
                    format!(
                        "https://tile.openstreetmap.org/{}/{}/{}.png",
                        index.z, index.x, index.y
                    )
                },
                galileo::TileSchema::web(18),
            );

        let view = galileo::MapView::new(
            &galileo::galileo_types::geo::impls::GeoPoint2d::latlon(42.4435, -123.3260),
            galileo::TileSchema::web(18).lod_resolution(13).unwrap(),
        );

        let tile_source = |index: &galileo::tile_scheme::TileIndex| {
            format!(
                "https://tile.openstreetmap.org/{}/{}/{}.png",
                index.z, index.x, index.y
            )
        };

        let layer = Box::new(galileo::MapBuilder::create_raster_tile_layer(
            tile_source,
            galileo::TileSchema::web(18),
        ));

        let messenger = galileo::winit::WinitMessenger::new(window);

        let map = Arc::new(RwLock::new(galileo::Map::new(
            view,
            vec![layer],
            Some(messenger),
        )));

        Self {
            event_processor,
            renderer,
            map,
        }
    }
}
