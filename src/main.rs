use bea_egui::{trace_init, App, Arrive};
use winit::event_loop;

#[tokio::main]
async fn main() -> Arrive<()> {
    trace_init();
    let event_loop = event_loop::EventLoop::<accesskit_winit::Event>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    event_loop.set_control_flow(event_loop::ControlFlow::Wait);

    let mut app = App::new(proxy);
    event_loop.run_app(&mut app)?;

    Ok(())
}
