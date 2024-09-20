use crate::{Act, Arrive, Cmd, Lens};
use rand::Rng;
use std::collections::HashMap;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::{
    dpi,
    event::{self, WindowEvent},
    event_loop, monitor, window,
};

/// The `app` module contains the `App` struct, which holds the parent-level top view of the
/// application state.
///
/// # Creating Windows with Tardy
///
/// The purpose of the `App` struct is to render content to the user in one or more windows.  There
/// is no content yet, because the [`winit`] library we are using to make windows does not do
/// anything else.  It is "a low-level brick in a hierarchy of libraries".
///
/// This code draws inspiration from the [window]("https://github.com/rust-windowing/winit/blob/master/examples/window.rs")
/// example in the [`winit`] repository (primary using \<CR-C\> and \<CR-V\>).  The main feature from
/// this example that I want to lift is the ability to create multiple windows.  Admittedly, I do
/// not have anything to put in these windows yet, but previously when I tackled this library, I
/// had trouble handling more than one window.  Today's goal: create as many windows as I want.
///
/// On April 27th of this year, the `winit` crate released version `0.30.0`, and I was eager to embrace the change.  
/// The examples I had pillaged my code from were all on a pre-0.30 version.  How hard could it be?
/// Turns out it boiled down to two key bits of *news* in the release notes:
///
/// * Deprecated EventLoop::run, use EventLoop::run_app.
/// * The new app APIs accept a newly added `ApplicationHandler<T>` instead of `Fn`
///
/// I.e. *Your old code will break*.  Some refactoring required.  This is a green-field space to
/// begin that refactoring process.
///
/// The second main goal of this refactor is make the application async from the ground up.  Too
/// many geospatial operations are compute heavy, and too many professional collaborations are over
/// network to run blocking operations.  Threads would be an acceptable alternative, but I am
/// interesting in biting into the shiny apple of async code, so here we go.
///
/// ## Update 0.1.1
///
/// The `App` struct now includes a `proxy` field holding the event loop proxy used to send events
/// from the async process back to the sync event loop as a user event of type `Hijinks`.
#[derive(Debug)]
pub struct App {
    cmd: Cmd,
    config: config::Config,
    proxy: event_loop::EventLoopProxy<accesskit_winit::Event>,
    windows: HashMap<window::WindowId, Lens>,
}

/// ### Fields
///
/// * The `cmd` field holds the [`Cmd`] struct, which maps keyboard inputs to program responses.
/// * The `config` field holds the [`config::Config`] loaded from `Tardy.toml`.
/// * The `proxy` fields holds the [`event_loop::EventLoopProxy`] that async processes use to send
///   [`Hijinks`] to the main event loop.
/// * The `windows` field holds a [`HashMap`] with keys of type [`window::WindowId`] and values of type [`Lens`].
impl App {
    /// Creates an instance of `App`.  Reads user key mappings from `Tardy.toml` using
    /// [`App::load_config`], then translates the mappings to commands using [`App::load_cmds`].
    ///
    /// ## Version 0.1.1. Update
    ///
    /// The `new` method now requires the user to provide a `proxy` input, specifying an
    /// [`event_loop::EventLoopProxy`], so that our async process can send events back to the main
    /// sync loop.  Since we tie up the main event loop when we run the application, we create a
    /// proxy on startup and store it in the `proxy` field of `App` for later use.  
    ///
    /// This later use occurs when summoning the [`ImpKing`], at which point we clone the proxy
    /// and pass it to the async process, making no further use of it within `App`.  As the top
    /// level data structure, we are using `App` to carry water from `main.rs` to a place where
    /// the async workers can drink it.
    pub fn new(proxy: event_loop::EventLoopProxy<accesskit_winit::Event>) -> Self {
        let cmd = Cmd::default();
        let config = config::Config::default();
        let windows = HashMap::new();
        let mut app = Self {
            cmd,
            config,
            proxy,
            windows,
        };
        app.load_config();
        app.load_cmds();
        app
    }
    /// Instead of using a `WindowBuilder`, we now create a default instance of
    /// [`window::WindowAttributes`], and modify it to be transparent and carry the title `Tardy`.
    /// Besides looking cool, `winit` recommends setting the window to transparent if you are not
    /// ready to render anything to the window yet, to prevent "garbage capture".  As our program will terminate before we are
    /// ready, this is the ideal setting.
    ///
    /// We pass the attributes to the [`event_loop::ActiveEventLoop::create_window`] method.
    /// Here we wrap the window in an [`Arc`] for no good reason, though I would certainly *like*
    /// to need an [`Arc`] here to render an `egui` menu on top of a GIS map.
    ///
    /// Finally, we create an instance of [`Lens`] from the window, and insert it as a value into
    /// the [`HashMap`] in the `windows` field, using the window id as a key.
    ///
    /// Will [`crate::Blame::EventLoop`] when [`event_loop::ActiveEventLoop::create_window`] fails.
    #[tracing::instrument(skip_all)]
    pub fn create_window(
        &mut self,
        event_loop: &event_loop::ActiveEventLoop,
        attributes: Option<window::WindowAttributes>,
    ) -> Arrive<()> {
        let attr = if let Some(attributes) = attributes {
            attributes
        } else {
            window::Window::default_attributes()
                .with_title("Tardy")
                .with_transparent(true)
        };
        let window = event_loop.create_window(attr)?;
        let window = Arc::new(window);
        // Did I create a window?
        tracing::trace!("Window created: {:?}", window.id());
        self.windows.insert(window.id(), Lens::new(window.clone()));
        // How many am I up to?
        tracing::trace!("Total windows: {}", self.windows.len());
        Ok(())
    }

    /// The user specifies key mappings in `Tardy.toml`, as described in the docs for [`Act`].
    /// I chose to use the [`config`] crate for parsing `toml`, as I'm likely to botch it if I
    /// tried to do it myself.  Here we call [`config::Config::builder`] and attempt to read in the
    /// source at `Tardy.toml`.
    ///
    /// If the build fails, we fall back on a default that happens to be exactly the same as my
    /// current `Tardy.toml`.  The current method has some drawbacks.  The default fallback would
    /// get onerous if I had more than two actions to worry about.  Also, I resort to unwrapping
    /// the default build, which will crash my program if it panics for some reason.
    #[tracing::instrument(skip_all)]
    pub fn load_config(&mut self) {
        if let Ok(config) = config::Config::builder()
            .add_source(config::File::with_name("config"))
            .build()
        {
            self.config = config;
            // Sanity check that the file read correctly.
            tracing::trace!("Config set from file.");
        } else {
            // Warn me the user config couldn't be read.
            tracing::warn!("Could not read config from file.");
            let config = config::Config::builder();
            let config = config.set_default("exit", "Escape").unwrap();
            let config = config.set_default("new_window", "n").unwrap();
            let config = config.build().unwrap();
            self.config = config;
        }

        // Read the config to make sure its correct.
        tracing::trace!("{:#?}", self.config);
    }

    /// Keys and values play reversed roles in the [`Cmd`] and [`config::Config`] structs.  Here we
    /// convert one to the other using the [`Cmd::from`] implementation.
    /// Failure to read any commands from the config will produce an empty [`Cmd`], which will
    /// restrict the user to mouse interactions.
    #[tracing::instrument(skip_all)]
    pub fn load_cmds(&mut self) {
        let cmd = Cmd::from(&self.config);
        self.cmd = cmd;
        tracing::trace!("Commands read from config.");
        // Do you see the commands you expected?
        tracing::trace!("{:?}", self.cmd);
    }

    /// The act method dispatches program responses based upon the variant of [`Act`] passed in the
    /// `act` argument. Takes a mutable reference to `Self` in order to create and remove windows
    /// from the `windows` field.  The `id` parameter identifies the window upon which to apply the
    /// action.  The `event_loop` provides a reference to the active event loop for new window
    /// creation.
    ///
    /// We match on `act` and dispatch to the appropriate handler, before returning `Ok`.
    /// Will [`crate::Blame::EventLoop`] if [`App::create_window`] fails.
    #[tracing::instrument(skip_all)]
    pub fn act(
        &mut self,
        act: &Act,
        id: &window::WindowId,
        event_loop: &event_loop::ActiveEventLoop,
    ) -> Arrive<()> {
        match act {
            Act::CloseWindow => {
                tracing::info!("Closing window.");
                let _ = self.windows.remove(id);
                Ok(())
            }
            Act::Exit => {
                tracing::trace!("Requesting exit.");
                self.windows.clear();
                Ok(())
            }
            Act::NewWindow => self.create_window(event_loop, None),
            Act::Be => {
                tracing::trace!("Taking it easy.");
                Ok(())
            }
        }
    }

    /// The `keyboard_input` method takes incoming keyboard presses and translates them to an [`Act`] variant using the [`Cmd::act`] method.
    /// If the key event passed in the `event` argument translates to a valid [`Act`], we pass it
    /// to the [`App::act`] method for handling.
    ///
    /// Takes a mutable reference to `self` to pass to [`App::act`].
    /// The `id` parameter indicates the active window and gets passed to [`App::act`].
    /// The `event_loop` parameter indicates the active event loop, and also gets passed to
    /// [`App::act`].
    /// Commits a `FauxPas` if [`App::act`] fails.
    #[tracing::instrument(skip_all)]
    pub fn keyboard_input(
        &mut self,
        id: &window::WindowId,
        event: &event::KeyEvent,
        event_loop: &event_loop::ActiveEventLoop,
    ) -> Arrive<()> {
        // Dispatch actions only on press.
        if event.state.is_pressed() {
            // Tell me I at least pressed the right key.
            tracing::trace!("Press detected: {:#?}", event);
            if let Some(act) = self.cmd.act(event) {
                // Helpful to know it triggered if the handler doesn't respond right.
                tracing::trace!("Act detected: {act}");
                self.act(&act, id, event_loop)?;
            } else {
                // No crime here.
                tracing::trace!("Invalid key.");
            }
        }
        Ok(())
    }

    /// The `screen_sizes` method returns a vector of physical sizes for each monitor available to
    /// the app.  The purpose of this function is to ascertain valid areas for drawing new windows.
    /// We avoid asking for windows with areas outside the size of the target screen.
    ///
    /// The [`winit`] library exposes the `available_monitors` method through the
    /// [`event_loop::EventLoop`] struct, and through a wrapper in the [`window::Window`] struct.
    /// This method runs after creation of the initial window, so we access the this window and
    /// call [`window::Window::available_monitors`], taking the first window we find because any
    /// will do.  For each available monitor, we collect the screen size using the
    /// [`monitor::MonitorHandle::size`] method.
    #[tracing::instrument(skip_all)]
    pub fn screen_sizes(&self) -> Option<Vec<dpi::PhysicalSize<u32>>> {
        if !self.windows.is_empty() {
            let values = self.windows.values().take(1).collect::<Vec<&Lens>>();
            let lens = values[0];
            let result = lens
                .window()
                .available_monitors()
                .map(|handle| handle.size())
                .collect::<Vec<dpi::PhysicalSize<u32>>>();
            tracing::info!("Monitors read.");
            Some(result)
        } else {
            tracing::warn!("No window available to measure.");
            None
        }
    }

    /// The `default_window_size` returns the size of the first window returned by calling
    /// [`HashMap::values`] on the [`HashMap`] in the `windows` field.  Note that if several
    /// windows exist, any one of them could return here.  In our program, we have only created an
    /// inital window using the default attributes.  On my machine, this produces a height of 600
    /// and a width of 800 in [`dpi::PhysicalSize<u32>`].  We measure the size of the window using
    /// the [`window::Window::outer_size`] method.
    ///
    /// Having never tried to change the size of a window, I was not really sure what format to
    /// expect.  Turns out, the [`window::Window::outer_size`] method returns a
    /// [`dpi::PhysicalSize<u32>`].  From this, I was able to infer that I should use the same
    /// struct to specify the sizes of new windows. Since monitors return their size in the same
    /// units, we can easily determine if a window's size will overrun the containing screen.
    #[tracing::instrument(skip_all)]
    pub fn default_window_size(&self) -> Option<dpi::PhysicalSize<u32>> {
        if !self.windows.is_empty() {
            let values = self.windows.values().take(1).collect::<Vec<&Lens>>();
            let lens = values[0];
            let result = lens.window().outer_size();
            tracing::info!("Window size measured.");
            Some(result)
        } else {
            tracing::warn!("No window available to measure.");
            None
        }
    }

    /// The `lenses` method creates a vector of references to the [`Lens`] values within the
    /// [`HashMap<window::WindowId, Lens>`] struct in the `windows` field.  The purpose of this
    /// method is to obtain a list of open windows in the application.
    ///
    /// Returns [`None`] if the [`HashMap`] in the `windows` field is empty.  Otherwise we call
    /// [`std::iter::Iterator::collect`] on [`HashMap::values`] to gather references to the
    /// windows, returned to the user as a vector.
    ///
    /// Called by [`App::monitors`] to get access to a window.
    #[tracing::instrument(skip_all)]
    pub fn lenses(&self) -> Option<Vec<&Lens>> {
        if !self.windows.is_empty() {
            let lens = self.windows.values().collect::<Vec<&Lens>>();
            tracing::info!("Lenses read.");
            Some(lens)
        } else {
            tracing::warn!("Could not read lenses.");
            None
        }
    }

    /// The `monitors` method reads the available monitors into a vector of type
    /// [`monitor::MonitorHandle`].
    ///
    /// Calls [`App::lenses`] to get a reference to an existing window, in order to get access to
    /// the [`window::Window::available_monitors`] method.  We collect the result into a vector of
    /// type [`monitor::MonitorHandle`].
    ///
    /// Called by [`App::random_monitor`] and [`App::random_monitors`].
    /// Returns [`None`] when [`App::lenses`] returns [`None`].
    #[tracing::instrument(skip_all)]
    pub fn monitors(&self) -> Option<Vec<monitor::MonitorHandle>> {
        if let Some(lenses) = self.lenses() {
            let monitors = lenses[0].window().available_monitors().collect();
            tracing::info!("Monitors read.");
            Some(monitors)
        } else {
            tracing::warn!("Could not read monitors.");
            None
        }
    }

    /// The `random_monitor` method selects a monitor at random from those available to the
    /// application.  The purpose of this method is to randomize the target monitor on which
    /// [`crate::Imp`] types will perform [`Hijinks`].
    ///
    /// Calls [`App::monitors`] to get a vector of available monitor handles.  Randomly selects an
    /// index along the vector and returns the selected [`monitor::MonitorHandle`].
    ///
    /// Called by [`App::frame`] to select a target monitor.
    /// Returns [`None`] when [`App::monitors`] returns [`None`].
    #[tracing::instrument(skip_all)]
    pub fn random_monitor(&self) -> Option<monitor::MonitorHandle> {
        if let Some(monitors) = self.monitors() {
            let mut rng = rand::thread_rng();
            let idx = rng.gen_range(0..monitors.len());
            tracing::info!("Monitor selected.");
            Some(monitors[idx].clone())
        } else {
            tracing::warn!("Could not select monitor.");
            None
        }
    }

    /// The `random_monitors` method selects `count` monitors at random from those available to the
    /// application.  The purpose of this method is to randomize the target monitors on which
    /// [`crate::Imp`] types will perform [`Hijinks`].
    ///
    /// The [`App::random_monitor`] method will call [`App::monitors`] once for each new monitor
    /// selection, whereas this method calls [`App::monitors`] once and reuses the vector for
    /// subsequent selections.  Since we currently only make [`crate::Imp`] types in batch, this is
    /// the method we use.
    ///
    /// Calls [`App::monitors`] to get a vector of available monitor handles.  Randomly selects
    /// indexes along the vector and returns a vector of the selected [`monitor::MonitorHandle`]
    /// types.
    ///
    /// Returns [`None`] when [`App::monitors`] returns [`None`].
    #[tracing::instrument(skip(self))]
    pub fn random_monitors(&self, count: usize) -> Option<Vec<monitor::MonitorHandle>> {
        if let Some(monitors) = self.monitors() {
            let mut rng = rand::thread_rng();
            let mut handles = Vec::new();
            for _ in 0..count {
                let idx = rng.gen_range(0..monitors.len());
                tracing::trace!("Monitor {} selected.", idx);
                handles.push(monitors[idx].clone());
            }
            tracing::info!("Monitors selected.");
            Some(handles)
        } else {
            tracing::warn!("Could not select monitors.");
            None
        }
    }

    /// The `frame` method creates a [`Frame`] from an available monitor.  The
    /// purpose of this method is to create a target screen, position and size for a new window.
    /// Since we create [`Frame`] types in batch, we elect to use [`App::frames`] instead.
    ///
    /// Calls [`App::random_monitor`] to select a target monitor, where a success returns a
    /// randomly-selected [`monitor::MonitorHandle`].  Using our [`From`] implementation for
    /// [`monitor::MonitorHandle`], we create a [`Frame`] from the handle and return it to the
    /// user.
    ///
    /// Returns [`None`] if [`App::random_monitor`] returns [`None`].
    #[tracing::instrument(skip_all)]
    pub fn frame(&self) -> Option<Frame> {
        if let Some(monitor) = self.random_monitor() {
            let frame = Frame::from(monitor);
            tracing::info!("Frame created.");
            Some(frame)
        } else {
            tracing::warn!("Could not create frame.");
            None
        }
    }

    /// The `frames` method creates a vector of type [`Frame`] from the available monitors.  The
    /// purpose of this method is to create a vector of target screens, positions and sizes for new windows to
    /// pass along to a [`crate::Imp`] for use in the [`crate::Imp::meddle`] method.
    ///
    /// Calls [`App::random_monitors`] to select target monitors, where a success returns a
    /// randomly-selected vector of type [`monitor::MonitorHandle`].  Using our [`From`] implementation for
    /// [`monitor::MonitorHandle`], we create a [`Frame`] from each handle and return it to the
    /// user.
    ///
    /// Called by [`App::imp_king`] to populate the `frames` field of the [`crate::ImpKing`].
    /// Returns [`None`] if [`App::random_monitors`] returns [`None`].
    #[tracing::instrument(skip(self))]
    pub fn frames(&self, count: usize) -> Option<Vec<Frame>> {
        if let Some(monitors) = self.random_monitors(count) {
            let frames = monitors
                .into_iter()
                .map(Frame::from)
                .collect::<Vec<Frame>>();
            tracing::info!("Frames created.");
            Some(frames)
        } else {
            tracing::warn!("Could not create frames.");
            None
        }
    }
}

/// The impl for `ApplicationHandler` is boiled down to as little as possible.
/// * The `resumed` method gets called once at startup when the program is ready
///   to make the initial window.  Calls [`App::create_window`] and unwraps it with an `expect`.
/// * The `window_event` method removes the current window on a [`WindowEvent::CloseRequested`].
///   It dispatches keyboard input from a [`WindowEvent::KeyboardInput`] to the [`App::keyboard_input`]
///   method, converting errors to trace level logs (hopefully they weren't important).
/// * The [`WindowEvent::RedrawRequested`] variant will trigger a [`window::Window::request_redraw`]
///   call if the `refresh` field on [`Lens`] is set to `true`, which it never is.
/// * We delegate program exit to the `about_to_wait` method, where we check to see if there are open
///   windows remaining.  If all windows are closed, we exit gracefully.
///
///   ## Version 0.1.1 Update
///
///   We have added an implementation of the [`ApplicationHandler::user_event`]
///   method on the [`Hijinks`] type.  The purpose of this method is to relay async events that
///   occur independently from the main event loop back into the sync event loop, using the
///   library-provided API for custom events.
///
///   In the windows example from the [`winit`] repository, the
///   authors use an mpsc channel to send user events to the application from another thread.
///   The main event loops access this event through the `proxy_wake_up` method.  However, the docs
///   for [`ApplicationHandler`] do not list this method.
///
///   Instead, I found the [`ApplicationHandler::user_event`] method, emitted when an event is sent
///   from [`event_loop::EventLoopProxy::send_event`].  The corresponding [docs]("https://docs.rs/winit/latest/winit/event_loop/index.html") helpfully indicate:
///
///   > If you want to send custom events to the event loop, use EventLoop::create_proxy to acquire an EventLoopProxy and call its send_event method.
///
///   The example code from the [`event_loop::EventLoop::create_proxy`] method includes an
///   interesting tidbit:
///
///   ```
///   let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
///   ```
///
///   In order to send events into the loop, we have to register the event with the loop on its
///   creation.  Here, the authors have used turbofish notation to specify the type of event as
///   `UserEvent`. We have amended our code in `main.rs` to include the [`Hijinks`] event.
///   We proceed to create a proxy, as in the example code:
///
///   ```
///   let event_loop = event_loop::EventLoop::<Hijinks>::with_user_event().build()?;
///   let proxy = event_loop.create_proxy();
///   ```
///
///   Happily, the event loop proxy is [`Clone`], so we can store it in the `proxy` field of `App`
///   and pass it to whatever process needs to instigate [`Hijinks`].  When [`Hijinks`] occurs, the
///   event loops handles the event using the [`App::user_event`] method, which provides access to
///   the [`Hijinks`] instance through the `event` parameter.
///
///   We match on the variant of [`Hijinks`] to determine program response:
///
///   * [`Hijinks::Meddle`] indicates a proxy action and contains an [`Act`] variant.
///     We match on the [`Act`] variant to determine response:
///     *  [`Act::CloseWindow`] - Respond by closing a random window, without regard to "owner".
///     *  [`Act::NewWindow`] - Respond by opening a new window. Contains a [`Frame`] specifying
///        position and window size.
///     * No further variants of [`Act`] participate in [`Hijinks`].
///   * [`Hijinks::Vandalize`] - Respond by logging the contained message as an INFO level trace.
///   * [`Hijinks::Filch`] - Respond by sending a vector of [`Frame`] instances to the filcher.
impl ApplicationHandler<accesskit_winit::Event> for App {
    #[tracing::instrument(skip_all)]
    fn resumed(&mut self, event_loop: &event_loop::ActiveEventLoop) {
        self.create_window(event_loop, None)
            .expect("Could not create window.");
    }

    #[tracing::instrument(skip_all)]
    fn user_event(
        &mut self,
        event_loop: &event_loop::ActiveEventLoop,
        event: accesskit_winit::Event,
    ) {
        tracing::info!("User event detected.");
        // match event {
        //     Hijinks::Meddle(meddle) => match meddle.act() {
        //         Act::CloseWindow => {
        //             tracing::info!("Close window received.");
        //             let keys = self
        //                 .windows
        //                 .keys()
        //                 .cloned()
        //                 .collect::<Vec<window::WindowId>>();
        //             if keys.len() > 1 {
        //                 let mut rng = rand::thread_rng();
        //                 let idx = rng.gen_range(0..keys.len());
        //                 self.windows.remove(&keys[idx]);
        //             } else {
        //                 tracing::info!("App refuses to close the last window.");
        //             }
        //         }
        //         Act::NewWindow => {
        //             if let Some(frame) = meddle.frame() {
        //                 tracing::info!("Creating window from imp.");
        //                 let position = frame.position();
        //                 let size = frame.size();
        //                 let attr = window::Window::default_attributes()
        //                     .with_title(meddle.title())
        //                     .with_transparent(true)
        //                     .with_position(*position)
        //                     .with_inner_size(*size);
        //                 self.create_window(event_loop, Some(attr)).unwrap();
        //             } else {
        //                 tracing::warn!("New window invocations should always include a frame.");
        //             }
        //         }
        //         _ => tracing::warn!("Imps can't send this type of act."),
        //     },
        //     Hijinks::Vandalize(msg) => tracing::info!(msg),
        //     Hijinks::Filch(filch) => {
        //         if let Some(frames) = self.frames(FRAMES) {
        //             let tx = filch.dissolve();
        //             tx.send(frames).unwrap();
        //         }
        //     }
        // }
    }

    #[tracing::instrument(skip_all)]
    fn window_event(
        &mut self,
        event_loop: &event_loop::ActiveEventLoop,
        id: window::WindowId,
        event: WindowEvent,
    ) {
        let window = match self.windows.get_mut(&id) {
            Some(window) => window,
            None => return,
        };

        match event {
            WindowEvent::CloseRequested => {
                tracing::trace!("Closing Window={id:?}");
                self.windows.remove(&id);
                tracing::trace!("Windows remaining: {}", self.windows.len());
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => {
                match self.keyboard_input(&id, &event, event_loop) {
                    Ok(_) => tracing::trace!("Event handled."),
                    Err(e) => tracing::trace!("Unexpected: {}", e.to_string()),
                };
            }
            WindowEvent::RedrawRequested => {
                // I left these comments in from the example to remind me to put some cool stuff
                // here later.
                //
                // Redraw the application.
                //
                // It's preferable for applications that do not render continuously to render in
                // this event rather than in AboutToWait, since rendering in here allows
                // the program to gracefully handle redraws requested by the OS.

                // Draw.

                // Queue a RedrawRequested event.
                //
                // You only need to call this if you've determined that you need to redraw in
                // applications which do not always need to. Applications that redraw continuously
                // can render here instead.
                if *window.refresh() {
                    window.window().request_redraw();
                    window.with_refresh(false);
                }
            }
            _ => (),
        }
    }

    #[tracing::instrument(skip_all)]
    fn about_to_wait(&mut self, event_loop: &event_loop::ActiveEventLoop) {
        if self.windows.is_empty() {
            tracing::trace!("No windows left, exiting...");
            event_loop.exit();
        }
    }
}

/// The `Frame` struct holds data for creating a new window.
///
/// * The `monitor` field contains the target [`monitor::MonitorHandle`].
/// * The `position` field contains the anchor position for placing the new window.
/// * The `size` field contains the size target for the new window.
///
/// The purpose of the `Frame` struct is to provide a unique position and size for new windows
/// created by [`Hijinks`].  When creating a new window, the default [`window::WindowAttributes`]
/// will create a window with the same size and location, such that they overlay each other, and it
/// is unclear how many windows are open.  [`Hijinks`] are less effective when they are difficult
/// to detect, so we randomize the screen, position and size of new windows to make them more
/// noticeable/annoying.
///
/// Determining the range of valid window sizes and positions, given the constraints of the
/// available monitor, occurs within the [`From`] implementation on [`monitor::MonitorHandle`]:
///
/// * Window height cannot exceed screen height less the margin of padding [`MIN_SPAN`].
/// * Window width cannot exceed screen width less the margin of padding [`MIN_SPAN`].
/// * Window position x cannot exceed screen width less window width.
/// * Window position y cannot exceed screen height less window height.
///
/// We select random values from the remaining ranges using [`rand::Rng::gen_range`], returning the
/// resulting values as a [`dpi::PhysicalPosition<u32>`].
#[derive(Debug, Clone, derive_new::new, derive_getters::Getters)]
pub struct Frame {
    monitor: monitor::MonitorHandle,
    position: dpi::PhysicalPosition<u32>,
    size: dpi::PhysicalSize<u32>,
}

impl From<monitor::MonitorHandle> for Frame {
    #[tracing::instrument]
    fn from(monitor: monitor::MonitorHandle) -> Self {
        // Sync only.
        let mut rng = rand::thread_rng();
        // Window must be within the monitor size.
        let monitor_size = monitor.size();
        // Generate random width and height within monitor size.
        let width = rng.gen_range(MIN_SPAN..(monitor_size.width - MIN_SPAN));
        let height = rng.gen_range(MIN_SPAN..(monitor_size.height - MIN_SPAN));
        // Create physical size from width and height.
        let size = dpi::PhysicalSize::new(width, height);
        // Do not let the window overhand the monitor space.
        let clip_x = monitor_size.width - size.width;
        let clip_y = monitor_size.height - size.height;
        // Generate random x and y within available space.
        let x = rng.gen_range(MIN_SPAN..clip_x);
        let y = rng.gen_range(MIN_SPAN..clip_y);
        // Create physical position from x and y.
        let position = dpi::PhysicalPosition::new(x, y);
        Self {
            monitor,
            position,
            size,
        }
    }
}

/// The `FRAME_POOL` constant determines the number of starting frames given to the
/// [`crate::ImpKing`] to distribute to [`crate::Imp`] types.
pub const FRAME_POOL: usize = 100;

/// The `FRAMES` constant determines the number of frames given to each [`crate::Imp`].
pub const FRAMES: usize = 10;

/// The `MIN_SPAN` constant serves as both the minimum size constraint for the height and width of
/// new windows, as well as the minimum padding between window and screen sizes.
/// Used to implement [`From<monitor::MonitorHandle>`] for [`Frame`].
pub const MIN_SPAN: u32 = 50;
