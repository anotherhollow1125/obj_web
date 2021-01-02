use obj_web::shader_settings::{
    model,
    light,
    ShaderState,
    shadowmap,
    camera,
    FileSourceBook,
};
#[cfg(target_arch = "wasm32")]
#[allow(unused)]
use obj_web::shader_settings::con_log;
use model::*;
use light::*;

use winit::{
    event::*,
    event_loop::{EventLoop, ControlFlow},
    window::{Window, WindowBuilder},
};

use anyhow::*;
use std::rc::Rc;

use cgmath::prelude::*;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = Date)]
    fn now() -> f64;

    /*
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
    */

    async fn load_file_js(s: &str) -> JsValue;
}

#[cfg(not(target_arch = "wasm32"))]
use once_cell::sync::Lazy;
#[cfg(not(target_arch = "wasm32"))]
static BASIC_TIME: Lazy<std::time::Instant> = Lazy::new(|| std::time::Instant::now());

fn now_sec() -> f32 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let n = std::time::Instant::now();
        let d = n.duration_since(*BASIC_TIME);
        d.as_secs_f32()
    }
    #[cfg(target_arch = "wasm32")]
    {
        now() as f32
    }
}

async fn run(
    event_loop: EventLoop<()>,
    window: Window,
    swapchain_format: wgpu::TextureFormat,
) {
    #[cfg(not(target_arch = "wasm32"))]
    println!("BASIC_TIME: {:?}", *BASIC_TIME);

    let file_source_book = {
        #[cfg(not(target_arch = "wasm32"))]
        {
            HashMap::new()
        }
        #[cfg(target_arch = "wasm32")]
        {
            prepare_files().await
        }
    };

    let state_w = ShaderState::new(
        &window,
        prepare_objects,
        swapchain_format,
        file_source_book,
    ).await;

    let mut state = match state_w {
        Ok(s) => s,
        Err(e) => {
            panic!("{:?}", e);
        },
    };

    let mut last_render_time = now_sec();
    let f = move || {
        event_loop.run(move |event, _, control_flow| {
            match event {
                Event::WindowEvent {
                    ref event,
                    window_id,
                } if window_id == window.id() && !state.input(event) => match event {
                    WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                    WindowEvent::KeyboardInput {
                        input,
                        ..
                    } => {
                        match input {
                            KeyboardInput {
                                state: ElementState::Pressed,
                                virtual_keycode: Some(VirtualKeyCode::Escape),
                                ..
                            } => *control_flow = ControlFlow::Exit,
                            _ => (),
                        }
                    },
                    WindowEvent::Resized(physical_size) => {
                        state.resize(*physical_size);
                    },
                    WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                        state.resize(**new_inner_size);
                    },
                    _ => (),
                },
                Event::RedrawRequested(_) => {
                    let now = now_sec();
                    let dt = now - last_render_time;
                    last_render_time = now;
    
                    state.update(dt, |s| {
    
                        let mut main_light = s.light_book[0].borrow_mut();
                        let old_position = main_light.position;
                        main_light.position =
                            cgmath::Quaternion::from_axis_angle((0.0, 1.0, 0.0).into(), cgmath::Deg(5.0))
                            * old_position;
                        s.light_buffer.update_light(&s.queue, &main_light);
    
                        let pos = main_light.position;
                        main_light.shadow.update(
                            Some(pos),
                            None,
                            &s.queue,
                            &mut s.shadow_uniform_buffer,
                        );
    
                        drop(main_light);
    
                        let mut sub_light = s.light_book[2].borrow_mut();
                        let old_position = sub_light.position;
                        sub_light.position =
                            cgmath::Quaternion::from_axis_angle((0.0, 1.0, 0.0).into(), cgmath::Deg(0.1))
                            * old_position;
                        s.light_buffer.update_light(&s.queue, &sub_light);
    
                        let pos = sub_light.position;
                        sub_light.shadow.update(
                            Some(pos),
                            None,
                            &s.queue,
                            &mut s.shadow_uniform_buffer,
                        );
    
                        /*
                        s.shadowmap.position = cgmath::Point3::from_vec(main_light.position);
                        s.shadowmap.direction = -main_light.position.normalize();
                        s.shadowmap.update_view_proj(&s.queue);
                        */
    
                        Ok(())
                    }).unwrap();
                    state.render();
                },
                Event::MainEventsCleared => {
                    window.request_redraw();
                },
                _ => (),
            }
        });
    };

    #[cfg(not(target = "wasm32"))]
    f();
    #[cfg(target = "wasm32")]
    {
        let f = Closure::wrap(Box::new(f) as Box<dyn FnMut()>);
        web_sys::window()
            .and_then(|win| {
                let _ = win.set_timeout_with_callback(f.as_ref().unchecked_ref())?;
            })
            .expect("couldn't set Timeout.");
        f.forget();
    }
}

fn prepare_objects(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sc_desc: &wgpu::SwapChainDescriptor,
    texture_layout: &wgpu::BindGroupLayout,
    instance_layout: &wgpu::BindGroupLayout,
    shadow_texture: &wgpu::Texture,
    file_source_book: &FileSourceBook,
) -> Result<(Vec<Instance>, Vec<Light>, Vec<Instance>)>
{
    use shadowmap::DirUpdateWay;
    use camera::Projection;

    let pos1 = (-5.0, 10.0, 5.0);
    let pos2 = (0.0, 2.1, 1.2);

    let shadow_1 = shadowmap::ShadowMap::new(
        0,
        pos1.into(),
        (0.0, 0.0, 0.0).into(), // don't use
        0.5,
        DirUpdateWay::SunLight {
            anchor_pos: (0.0, 0.0, 0.0).into(),
        },
        Projection::new(sc_desc.width, sc_desc.height, cgmath::Deg(45.0), 0.1, 100.0),
        device,
        queue,
        sc_desc,
        instance_layout,
        shadow_texture,
    );

    let spot_light_dir = (0.0, -1.0, 0.0);
    let shadow_2 = shadowmap::ShadowMap::new(
        1,
        pos2.into(),
        spot_light_dir.into(),
        0.0,
        DirUpdateWay::SpotLight,
        Projection::new(sc_desc.width, sc_desc.height, cgmath::Deg(120.0), 0.1, 100.0),
        device,
        queue,
        sc_desc,
        instance_layout,
        shadow_texture,
    );

    let pos3 = (-5.0, 10.0, -5.0);
    let shadow_3 = shadowmap::ShadowMap::new(
        2,
        pos3.into(),
        (0.0, 0.0, 0.0).into(), // don't use
        0.5,
        DirUpdateWay::SunLight {
            anchor_pos: (0.0, 0.0, 0.0).into(),
        },
        Projection::new(sc_desc.width, sc_desc.height, cgmath::Deg(45.0), 0.1, 100.0),
        device,
        queue,
        sc_desc,
        instance_layout,
        shadow_texture,
    );

    let lights = vec![
        Light::new(0, pos1.into(), (1.0, 1.0, 1.0).into(), 0.4, 1.0, shadow_1),
        Light::new_spotlight(
            1, pos2.into(), (1.0, 1.0, 0.0).into(),
            1.0, // intensity
            0.42,
            0.99, // inner
            0.85, // outer
            spot_light_dir.into(),
            shadow_2
        ),
        Light::new(2, pos3.into(), (0.0, 0.0, 1.0).into(), 0.4, 1.0, shadow_3),
    ];

    // let assets_dir = std::path::Path::new(env!("OUT_DIR")).join("assets");
    let assets_dir = std::path::Path::new(".").join("assets");
    let house2 = Model::load(
        0, device, queue, texture_layout,
        assets_dir.join("house2.obj"),
        file_source_book,
    )?;
    let house2 = Rc::new(house2);
    
    let house_i = Model::instantiate(
        house2.clone(),
        "house2".to_string(),
        (0.0, 0.0, 0.0).into(),
        cgmath::Quaternion::from_axis_angle(
            cgmath::Vector3::unit_z(),
            cgmath::Deg(0.0)
        ),
        1.0
    );

    let bulb = Model::load(
        1, device, queue, texture_layout,
        assets_dir.join("bulb.obj"),
        file_source_book,
    )?;
    let bulb = Rc::new(bulb);

    let bulb_i = Model::instantiate(
        bulb.clone(),
        "bulb".to_string(),
        (0.0, 2.08, 1.2).into(),
        cgmath::Quaternion::from_axis_angle(
            cgmath::Vector3::unit_z(),
            cgmath::Deg(0.0)
        ),
        0.042
    );

    Ok((
        vec![house_i],
        lights,
        vec![bulb_i]
    ))
}

use std::collections::HashMap;

#[cfg(target_arch = "wasm32")]
async fn load_file(s: &str) -> Vec<u8> {
    load_file_js(s).await.into_serde().unwrap()
}

#[cfg(target_arch = "wasm32")]
macro_rules! load_tuple {
    ($x:expr) => {
        ($x.to_string(), load_file($x).await)
    };
}

#[cfg(target_arch = "wasm32")]
async fn prepare_files() -> HashMap<String, Vec<u8>> {
    vec![
        load_tuple!("./assets/bulb.mtl"),
        load_tuple!("./assets/bulb.obj"),
        load_tuple!("./assets/default_texture.png"),
        load_tuple!("./assets/house.png"),
        load_tuple!("./assets/house2.mtl"),
        load_tuple!("./assets/house2.obj"),
    ].into_iter()
    .collect()
}

fn main() -> Result<()> {
    env_logger::init();
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Obj File Viewer: Snow theme.")
        .build(&event_loop)
        .unwrap();

    #[cfg(not(target_arch = "wasm32"))]
    {
        use futures::executor::block_on;
        block_on(run(event_loop, window, wgpu::TextureFormat::Bgra8UnormSrgb));
    }

    #[cfg(target_arch = "wasm32")]
    {
        std::panic::set_hook(Box::new(console_error_panic_hook::hook));
        // console_log::init().expect("could not initialize logger");
        use winit::platform::web::WindowExtWebSys;
        web_sys::window()
            .and_then(|win| win.document())
            .and_then(|doc| doc.body())
            .and_then(|body| {
                body.append_child(&web_sys::Element::from(window.canvas()))
                    .ok()
            })
            .expect("couldn't append canvas to document body");
        wasm_bindgen_futures::spawn_local(run(event_loop, window, wgpu::TextureFormat::Bgra8Unorm));
    }

    Ok(())
}