#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use ayagami_demo::AyagamiTestApp;

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    egui_logger::builder()
        .max_level(log::LevelFilter::Info)
        .init()
        .unwrap();

    let mut native_options = eframe::NativeOptions::default();
    native_options
        .wgpu_options
        .surface
        .desired_maximum_frame_latency = Some(1);
    eframe::run_native(
        "Ayagami Demo App",
        native_options,
        Box::new(|cc| Ok(Box::new(AyagamiTestApp::new(cc)))),
    )
    .unwrap();
}

// When compiling to web using trunk:
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    egui_logger::builder()
        .max_level(log::LevelFilter::Info)
        .init()
        .unwrap();

    //eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id was not a HtmlCanvasElement");

        let start_result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(AyagamiTestApp::new(cc)))),
            )
            .await;

        // Remove the loading text and spinner:
        if let Some(loading_text) = document.get_element_by_id("loading_text") {
            match start_result {
                Ok(_) => {
                    loading_text.remove();
                }
                Err(e) => {
                    loading_text.set_inner_html(
                        "<p> The app has crashed. See the developer console for details. </p>",
                    );
                    panic!("Failed to start eframe: {e:?}");
                }
            }
        }
    });
}
