//! # Handle the layer_shell in a winit way
//!
//! Min example is under
//!
//! ```rust, no_run
//! use std::fs::File;
//! use std::os::fd::AsFd;
//!
//! use layershellev::keyboard::{KeyCode, PhysicalKey};
//! use layershellev::reexport::*;
//! use layershellev::*;
//!
//! fn main() {
//!     let mut ev: WindowState<()> = WindowState::new("Hello")
//!         .with_single(false)
//!         .with_size((0, 400))
//!         .with_layer(Layer::Top)
//!         .with_margin((20, 20, 100, 20))
//!         .with_anchor(Anchor::Bottom | Anchor::Left | Anchor::Right)
//!         .with_keyboard_interacivity(KeyboardInteractivity::Exclusive)
//!         .with_exclusize_zone(-1)
//!         .build()
//!         .unwrap();
//!
//!     let mut virtual_keyboard_manager = None;
//!     ev.running(|event, ev, index| {
//!         match event {
//!             // NOTE: this will send when init, you can request bind extra object from here
//!             LayerEvent::InitRequest => ReturnData::RequestBind,
//!             LayerEvent::BindProvide(globals, qh) => {
//!                 // NOTE: you can get implied wayland object from here
//!                 virtual_keyboard_manager = Some(
//!                     globals
//!                         .bind::<zwp_virtual_keyboard_v1::ZwpVirtualKeyboardManagerV1, _, _>(
//!                             qh,
//!                             1..=1,
//!                             (),
//!                         )
//!                         .unwrap(),
//!                 );
//!                 println!("{:?}", virtual_keyboard_manager);
//!                 ReturnData::None
//!             }
//!             LayerEvent::XdgInfoChanged(_) => {
//!                 let index = index.unwrap();
//!                 let unit = ev.get_unit_with_id(index).unwrap();
//!                 println!("{:?}", unit.get_xdgoutput_info());
//!                 ReturnData::None
//!             }
//!             LayerEvent::RequestBuffer(file, shm, qh, init_w, init_h) => {
//!                 draw(file, (init_w, init_h));
//!                 let pool = shm.create_pool(file.as_fd(), (init_w * init_h * 4) as i32, qh, ());
//!                 ReturnData::WlBuffer(pool.create_buffer(
//!                     0,
//!                     init_w as i32,
//!                     init_h as i32,
//!                     (init_w * 4) as i32,
//!                     wl_shm::Format::Argb8888,
//!                     qh,
//!                     (),
//!                 ))
//!             }
//!             LayerEvent::RequestMessages(DispatchMessage::RequestRefresh { width, height, .. }) => {
//!                 println!("{width}, {height}");
//!                 ReturnData::None
//!             }
//!             LayerEvent::RequestMessages(DispatchMessage::MouseButton { .. }) => ReturnData::None,
//!             LayerEvent::RequestMessages(DispatchMessage::MouseEnter {
//!                 serial, pointer, ..
//!             }) => ReturnData::RequestSetCursorShape((
//!                 "crosshair".to_owned(),
//!                 pointer.clone(),
//!                 *serial,
//!             )),
//!             LayerEvent::RequestMessages(DispatchMessage::MouseMotion {
//!                 time,
//!                 surface_x,
//!                 surface_y,
//!             }) => {
//!                 println!("{time}, {surface_x}, {surface_y}");
//!                 ReturnData::None
//!             }
//!             LayerEvent::RequestMessages(DispatchMessage::KeyboardInput { event, .. }) => {
//!                if let PhysicalKey::Code(KeyCode::Escape) = event.physical_key {
//!                    ReturnData::RequestExist
//!                } else {
//!                    ReturnData::None
//!                }
//!            }
//!             _ => ReturnData::None,
//!         }
//!     })
//!     .unwrap();
//! }
//!
//! fn draw(tmp: &mut File, (buf_x, buf_y): (u32, u32)) {
//!     use std::{cmp::min, io::Write};
//!     let mut buf = std::io::BufWriter::new(tmp);
//!     for y in 0..buf_y {
//!         for x in 0..buf_x {
//!             let a = 0xFF;
//!             let r = min(((buf_x - x) * 0xFF) / buf_x, ((buf_y - y) * 0xFF) / buf_y);
//!             let g = min((x * 0xFF) / buf_x, ((buf_y - y) * 0xFF) / buf_y);
//!             let b = min(((buf_x - x) * 0xFF) / buf_x, (y * 0xFF) / buf_y);
//!
//!             let color = (a << 24) + (r << 16) + (g << 8) + b;
//!             buf.write_all(&color.to_ne_bytes()).unwrap();
//!         }
//!     }
//!     buf.flush().unwrap();
//! }
//! ```
//!
pub use events::NewLayerShellSettings;
pub use events::NewPopUpSettings;
use sctk::reexports::calloop::LoopHandle;
pub use waycrate_xkbkeycode::keyboard;
pub use waycrate_xkbkeycode::xkb_keyboard;

pub use sctk::reexports::calloop;

mod events;
mod strtoshape;

use events::DispatchMessageInner;

pub mod id;

pub use events::{AxisScroll, DispatchMessage, LayerEvent, ReturnData, XdgInfoChangedType};

use strtoshape::str_to_shape;
use wayland_client::{
    delegate_noop,
    globals::{registry_queue_init, BindError, GlobalError, GlobalList, GlobalListContents},
    protocol::{
        wl_buffer::WlBuffer,
        wl_compositor::WlCompositor,
        wl_display::WlDisplay,
        wl_keyboard::{self, KeyState, KeymapFormat, WlKeyboard},
        wl_output::{self, WlOutput},
        wl_pointer::{self, WlPointer},
        wl_registry,
        wl_seat::{self, WlSeat},
        wl_shm::WlShm,
        wl_shm_pool::WlShmPool,
        wl_surface::WlSurface,
        wl_touch::{self, WlTouch},
    },
    ConnectError, Connection, Dispatch, DispatchError, EventQueue, Proxy, QueueHandle, WEnum,
};

use sctk::reexports::{calloop::EventLoop, calloop_wayland_source::WaylandSource};

use std::time::Duration;

use wayland_cursor::{CursorImageBuffer, CursorTheme};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, ZwlrLayerSurfaceV1},
};

use wayland_protocols::xdg::shell::client::{
    xdg_popup::{self, XdgPopup},
    xdg_positioner::XdgPositioner,
    xdg_surface::{self, XdgSurface},
    xdg_wm_base::XdgWmBase,
};

use wayland_protocols::{
    wp::fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{self, WpFractionalScaleV1},
    },
    xdg::xdg_output::zv1::client::{
        zxdg_output_manager_v1::ZxdgOutputManagerV1,
        zxdg_output_v1::{self, ZxdgOutputV1},
    },
};

use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1::WpCursorShapeDeviceV1,
    wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
};

use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

use sctk::reexports::calloop::Error as CallLoopError;

#[derive(Debug, thiserror::Error)]
pub enum LayerEventError {
    #[error("connect error")]
    ConnectError(#[from] ConnectError),
    #[error("Global Error")]
    GlobalError(#[from] GlobalError),
    #[error("Bind Error")]
    BindError(#[from] BindError),
    #[error("Error during queue")]
    DispatchError(#[from] DispatchError),
    #[error("create file failed")]
    TempFileCreateFailed(#[from] std::io::Error),
    #[error("Event Loop Error")]
    EventLoopInitError(#[from] CallLoopError),
}

pub mod reexport {
    pub use wayland_protocols_wlr::layer_shell::v1::client::{
        zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
        zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity},
    };
    pub mod wl_shm {
        pub use wayland_client::protocol::wl_shm::Format;
        pub use wayland_client::protocol::wl_shm::WlShm;
    }
    pub mod zwp_virtual_keyboard_v1 {
        pub use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
            zwp_virtual_keyboard_manager_v1::{self, ZwpVirtualKeyboardManagerV1},
            zwp_virtual_keyboard_v1::{self, ZwpVirtualKeyboardV1},
        };
    }
    pub mod wp_fractional_scale_v1 {
        pub use wayland_protocols::wp::fractional_scale::v1::client::{
            wp_fractional_scale_manager_v1::{self, WpFractionalScaleManagerV1},
            wp_fractional_scale_v1::{self, WpFractionalScaleV1},
        };
    }
    pub mod wayland_client {
        pub use wayland_client::{
            globals::GlobalList,
            protocol::{
                wl_keyboard::{self, KeyState},
                wl_pointer::{self, ButtonState},
                wl_seat::WlSeat,
            },
            QueueHandle, WEnum,
        };
    }
}

#[derive(Debug)]
struct BaseState;

// so interesting, it is just need to invoke once, it just used to get the globals
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for BaseState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: <wl_registry::WlRegistry as wayland_client::Proxy>::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

/// this struct store the xdg_output information
#[derive(Debug)]
pub struct ZxdgOutputInfo {
    zxdgoutput: ZxdgOutputV1,
    logical_size: (i32, i32),
    position: (i32, i32),
}

impl ZxdgOutputInfo {
    fn new(zxdgoutput: ZxdgOutputV1) -> Self {
        Self {
            zxdgoutput,
            logical_size: (0, 0),
            position: (0, 0),
        }
    }

    /// you can get the Logic position of the screen current surface in
    pub fn get_position(&self) -> (i32, i32) {
        self.position
    }

    /// you can get the LogicalPosition of the screen current surface in
    pub fn get_logical_size(&self) -> (i32, i32) {
        self.logical_size
    }
}

/// This is the unit, binding to per screen.
/// Because layer_shell is so unique, on surface bind to only one
/// wl_output, only one buffer, only one output, so it will store
/// includes the information of ZxdgOutput, size, and layer_shell
///
/// and it can set a binding, you to store the related data. like
/// a cario_context, which is binding to the buffer on the wl_surface.

#[derive(Debug)]
enum Shell {
    LayerShell(ZwlrLayerSurfaceV1),
    PopUp((XdgPopup, XdgSurface)),
}

impl PartialEq<ZwlrLayerSurfaceV1> for Shell {
    fn eq(&self, other: &ZwlrLayerSurfaceV1) -> bool {
        match self {
            Self::LayerShell(shell) => shell == other,
            _ => false,
        }
    }
}

impl PartialEq<XdgPopup> for Shell {
    fn eq(&self, other: &XdgPopup) -> bool {
        match self {
            Self::PopUp((popup, _)) => popup == other,
            _ => false,
        }
    }
}

impl Shell {
    fn destroy(&self) {
        match self {
            Self::PopUp((popup, xdg_surface)) => {
                popup.destroy();
                xdg_surface.destroy();
            }
            Self::LayerShell(shell) => shell.destroy(),
        }
    }

    fn is_popup(&self) -> bool {
        matches!(self, Self::PopUp(_))
    }
}

#[derive(Debug)]
pub struct WindowStateUnit<T> {
    id: id::Id,
    display: WlDisplay,
    wl_surface: WlSurface,
    size: (u32, u32),
    buffer: Option<WlBuffer>,
    shell: Shell,
    zxdgoutput: Option<ZxdgOutputInfo>,
    fractional_scale: Option<WpFractionalScaleV1>,
    wl_output: Option<WlOutput>,
    binding: Option<T>,
    becreated: bool,
}

impl<T> WindowStateUnit<T> {
    fn is_popup(&self) -> bool {
        self.shell.is_popup()
    }
}

impl<T> WindowStateUnit<T> {
    /// get the WindowState id
    pub fn id(&self) -> id::Id {
        self.id
    }

    /// gen the WindowState [WindowWrapper]
    pub fn gen_wrapper(&self) -> WindowWrapper {
        WindowWrapper {
            id: self.id,
            display: self.display.clone(),
            wl_surface: self.wl_surface.clone(),
        }
    }
}
impl<T> WindowStateUnit<T> {
    #[inline]
    pub fn raw_window_handle_rwh_06(&self) -> Result<rwh_06::RawWindowHandle, rwh_06::HandleError> {
        Ok(rwh_06::WaylandWindowHandle::new({
            let ptr = self.wl_surface.id().as_ptr();
            std::ptr::NonNull::new(ptr as *mut _).expect("wl_surface will never be null")
        })
        .into())
    }

    #[inline]
    pub fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
        Ok(rwh_06::WaylandDisplayHandle::new({
            let ptr = self.display.id().as_ptr();
            std::ptr::NonNull::new(ptr as *mut _).expect("wl_proxy should never be null")
        })
        .into())
    }
}

impl<T> rwh_06::HasWindowHandle for WindowStateUnit<T> {
    fn window_handle(&self) -> Result<rwh_06::WindowHandle<'_>, rwh_06::HandleError> {
        let raw = self.raw_window_handle_rwh_06()?;

        // SAFETY: The window handle will never be deallocated while the window is alive,
        // and the main thread safety requirements are upheld internally by each platform.
        Ok(unsafe { rwh_06::WindowHandle::borrow_raw(raw) })
    }
}

impl<T> rwh_06::HasDisplayHandle for WindowStateUnit<T> {
    fn display_handle(&self) -> Result<rwh_06::DisplayHandle<'_>, rwh_06::HandleError> {
        let raw = self.raw_display_handle_rwh_06()?;

        // SAFETY: The window handle will never be deallocated while the window is alive,
        // and the main thread safety requirements are upheld internally by each platform.
        Ok(unsafe { rwh_06::DisplayHandle::borrow_raw(raw) })
    }
}

// if is only one window, use it will be easy
impl<T> rwh_06::HasWindowHandle for WindowState<T> {
    fn window_handle(&self) -> Result<rwh_06::WindowHandle<'_>, rwh_06::HandleError> {
        let raw = self.main_window().raw_window_handle_rwh_06()?;

        // SAFETY: The window handle will never be deallocated while the window is alive,
        // and the main thread safety requirements are upheld internally by each platform.
        Ok(unsafe { rwh_06::WindowHandle::borrow_raw(raw) })
    }
}

// if is only one window, use it will be easy
impl<T> rwh_06::HasDisplayHandle for WindowState<T> {
    fn display_handle(&self) -> Result<rwh_06::DisplayHandle<'_>, rwh_06::HandleError> {
        let raw = self.main_window().raw_display_handle_rwh_06()?;

        // SAFETY: The window handle will never be deallocated while the window is alive,
        // and the main thread safety requirements are upheld internally by each platform.
        Ok(unsafe { rwh_06::DisplayHandle::borrow_raw(raw) })
    }
}
impl<T> WindowStateUnit<T> {
    /// get the wl surface from WindowState
    pub fn get_wlsurface(&self) -> &WlSurface {
        &self.wl_surface
    }

    /// get the xdg_output info related to this unit
    pub fn get_xdgoutput_info(&self) -> Option<&ZxdgOutputInfo> {
        self.zxdgoutput.as_ref()
    }

    /// set the anchor of the current unit. please take the simple.rs as reference
    pub fn set_anchor(&self, anchor: Anchor) {
        if let Shell::LayerShell(layer_shell) = &self.shell {
            layer_shell.set_anchor(anchor);
            self.wl_surface.commit();
        }
    }

    /// you can reset the margin which bind to the surface
    pub fn set_margin(&self, (top, right, bottom, left): (i32, i32, i32, i32)) {
        if let Shell::LayerShell(layer_shell) = &self.shell {
            layer_shell.set_margin(top, right, bottom, left);
            self.wl_surface.commit();
        }
    }

    pub fn set_layer(&self, layer: Layer) {
        if let Shell::LayerShell(layer_shell) = &self.shell {
            layer_shell.set_layer(layer);
            self.wl_surface.commit();
        }
    }

    /// set the layer size of current unit
    pub fn set_size(&self, (width, height): (u32, u32)) {
        if let Shell::LayerShell(layer_shell) = &self.shell {
            layer_shell.set_size(width, height);
            self.wl_surface.commit();
        }
    }

    /// set current exclusive_zone
    pub fn set_exclusive_zone(&self, zone: i32) {
        if let Shell::LayerShell(layer_shell) = &self.shell {
            layer_shell.set_exclusive_zone(zone);
            self.wl_surface.commit();
        }
    }

    /// you can use this function to set a binding data. the message passed back contain
    /// a index, you can use that to get the unit. It will be very useful, because you can
    /// use the binding data to operate the file binding to the buffer. you can take
    /// startcolorkeyboard as reference.
    pub fn set_binding(&mut self, binding: T) {
        self.binding = Some(binding);
    }

    /// return the binding data, with mut reference
    pub fn get_binding_mut(&mut self) -> Option<&mut T> {
        self.binding.as_mut()
    }

    /// get the binding data
    pub fn get_binding(&self) -> Option<&T> {
        self.binding.as_ref()
    }

    /// get the size of the surface
    pub fn get_size(&self) -> (u32, u32) {
        self.size
    }

    /// this function will refresh whole surface. it will reattach the buffer, and damage whole,
    /// and final commit
    pub fn request_refresh(&self, (width, height): (i32, i32)) {
        self.wl_surface.attach(self.buffer.as_ref(), 0, 0);
        self.wl_surface.damage(0, 0, width, height);
        self.wl_surface.commit();
    }
}

/// main state, store the main information
#[derive(Debug)]
pub struct WindowState<T> {
    outputs: Vec<(u32, wl_output::WlOutput)>,
    current_surface: Option<WlSurface>,
    is_single: bool,
    units: Vec<WindowStateUnit<T>>,
    message: Vec<(Option<id::Id>, DispatchMessageInner)>,

    connection: Option<Connection>,
    event_queue: Option<EventQueue<WindowState<T>>>,
    wl_compositor: Option<WlCompositor>,
    xdg_output_manager: Option<ZxdgOutputManagerV1>,
    wmbase: Option<XdgWmBase>,
    shm: Option<WlShm>,
    cursor_manager: Option<WpCursorShapeManagerV1>,
    fractional_scale_manager: Option<WpFractionalScaleManagerV1>,
    globals: Option<GlobalList>,

    // base managers
    seat: Option<WlSeat>,
    keyboard_state: Option<xkb_keyboard::KeyboardState>,

    pointer: Option<WlPointer>,
    touch: Option<WlTouch>,
    virtual_keyboard: Option<ZwpVirtualKeyboardV1>,

    // states
    namespace: String,
    keyboard_interactivity: zwlr_layer_surface_v1::KeyboardInteractivity,
    anchor: Anchor,
    layer: Layer,
    size: Option<(u32, u32)>,
    exclusive_zone: Option<i32>,
    margin: Option<(i32, i32, i32, i32)>,

    // settings
    use_display_handle: bool,
    loop_handler: Option<LoopHandle<'static, Self>>,

    last_unit_index: usize,
    last_wloutput: Option<WlOutput>,

    return_data: Vec<ReturnData<T>>,
}

impl<T> WindowState<T> {
    pub fn append_return_data(&mut self, data: ReturnData<T>) {
        self.return_data.push(data);
    }
    /// remove a shell, destroy the surface
    pub fn remove_shell(&mut self, id: id::Id) {
        let Some(index) = self
            .units
            .iter()
            .position(|unit| unit.id == id && unit.becreated)
        else {
            return;
        };

        self.units[index].shell.destroy();
        self.units[index].wl_surface.destroy();

        if let Some(buffer) = self.units[index].buffer.as_ref() {
            buffer.destroy()
        }
        self.units.remove(index);
    }

    /// forget the remembered last output, next time it will get the new activated output to set the
    /// layershell
    pub fn forget_last_output(&mut self) {
        self.last_wloutput.take();
    }
}

/// Simple WindowState, without any data binding or info
pub type WindowStateSimple = WindowState<()>;

impl<T> WindowState<T> {
    // return the first window
    // I will use it in iced
    pub fn main_window(&self) -> &WindowStateUnit<T> {
        &self.units[0]
    }

    /// use iced id to find WindowStateUnit
    pub fn get_window_with_id(&self, id: id::Id) -> Option<&WindowStateUnit<T>> {
        self.units.iter().find(|w| w.id() == id)
    }
    // return all windows
    pub fn windows(&self) -> &Vec<WindowStateUnit<T>> {
        &self.units
    }
}

#[derive(Debug)]
pub struct WindowWrapper {
    pub id: id::Id,
    display: WlDisplay,
    wl_surface: WlSurface,
}

impl WindowWrapper {
    pub fn id(&self) -> id::Id {
        self.id
    }
}

impl<T> WindowState<T> {
    /// get a seat from state
    pub fn get_seat(&self) -> &WlSeat {
        self.seat.as_ref().unwrap()
    }

    /// get the keyboard
    pub fn get_keyboard(&self) -> Option<&WlKeyboard> {
        Some(&self.keyboard_state.as_ref()?.keyboard)
    }

    /// get the pointer
    pub fn get_pointer(&self) -> Option<&WlPointer> {
        self.pointer.as_ref()
    }

    /// get the touch
    pub fn get_touch(&self) -> Option<&WlTouch> {
        self.touch.as_ref()
    }
}

impl<T> WindowState<T> {
    /// gen the wrapper to the main window
    /// used to get display and etc
    pub fn gen_main_wrapper(&self) -> WindowWrapper {
        self.main_window().gen_wrapper()
    }
}

impl WindowWrapper {
    #[inline]
    pub fn raw_window_handle_rwh_06(&self) -> Result<rwh_06::RawWindowHandle, rwh_06::HandleError> {
        Ok(rwh_06::WaylandWindowHandle::new({
            let ptr = self.wl_surface.id().as_ptr();
            std::ptr::NonNull::new(ptr as *mut _).expect("wl_surface will never be null")
        })
        .into())
    }

    #[inline]
    pub fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
        Ok(rwh_06::WaylandDisplayHandle::new({
            let ptr = self.display.id().as_ptr();
            std::ptr::NonNull::new(ptr as *mut _).expect("wl_proxy should never be null")
        })
        .into())
    }
}
impl rwh_06::HasWindowHandle for WindowWrapper {
    fn window_handle(&self) -> Result<rwh_06::WindowHandle<'_>, rwh_06::HandleError> {
        let raw = self.raw_window_handle_rwh_06()?;

        // SAFETY: The window handle will never be deallocated while the window is alive,
        // and the main thread safety requirements are upheld internally by each platform.
        Ok(unsafe { rwh_06::WindowHandle::borrow_raw(raw) })
    }
}

impl rwh_06::HasDisplayHandle for WindowWrapper {
    fn display_handle(&self) -> Result<rwh_06::DisplayHandle<'_>, rwh_06::HandleError> {
        let raw = self.raw_display_handle_rwh_06()?;

        // SAFETY: The window handle will never be deallocated while the window is alive,
        // and the main thread safety requirements are upheld internally by each platform.
        Ok(unsafe { rwh_06::DisplayHandle::borrow_raw(raw) })
    }
}

impl<T> WindowState<T> {
    /// create a WindowState, you need to pass a namespace in
    pub fn new(namespace: &str) -> Self {
        assert_ne!(namespace, "");
        Self {
            namespace: namespace.to_owned(),
            ..Default::default()
        }
    }

    /// if the shell is a single one, only display on one screen,
    /// fi true, the layer will binding to current screen
    pub fn with_single(mut self, single: bool) -> Self {
        self.is_single = single;
        self
    }

    /// keyboard_interacivity, please take look at [layer_shell](https://wayland.app/protocols/wlr-layer-shell-unstable-v1)
    pub fn with_keyboard_interacivity(
        mut self,
        keyboard_interacivity: zwlr_layer_surface_v1::KeyboardInteractivity,
    ) -> Self {
        self.keyboard_interactivity = keyboard_interacivity;
        self
    }

    /// set the layer_shell anchor
    pub fn with_anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// set the layer_shell layer
    pub fn with_layer(mut self, layer: Layer) -> Self {
        self.layer = layer;
        self
    }

    /// set the layer margin
    pub fn with_margin(mut self, (top, right, bottom, left): (i32, i32, i32, i32)) -> Self {
        self.margin = Some((top, right, bottom, left));
        self
    }

    /// if not set, it will be the size suggested by layer_shell, like anchor to four ways,
    /// and margins to 0,0,0,0 , the size will be the size of screen.
    ///
    /// if set, layer_shell will use the size you set
    pub fn with_size(mut self, size: (u32, u32)) -> Self {
        self.size = Some(size);
        self
    }

    /// set the window size, optional
    pub fn with_option_size(mut self, size: Option<(u32, u32)>) -> Self {
        self.size = size;
        self
    }

    /// exclusive_zone, please take look at [layer_shell](https://wayland.app/protocols/wlr-layer-shell-unstable-v1)
    pub fn with_exclusize_zone(mut self, exclusive_zone: i32) -> Self {
        self.exclusive_zone = Some(exclusive_zone);
        self
    }

    /// set layershellev to use display_handle
    pub fn with_use_display_handle(mut self, use_display_handle: bool) -> Self {
        self.use_display_handle = use_display_handle;
        self
    }
}

impl<T> Default for WindowState<T> {
    fn default() -> Self {
        Self {
            outputs: Vec::new(),
            current_surface: None,
            is_single: true,
            units: Vec::new(),
            message: Vec::new(),

            connection: None,
            event_queue: None,
            wl_compositor: None,
            shm: None,
            wmbase: None,
            cursor_manager: None,
            xdg_output_manager: None,
            globals: None,
            fractional_scale_manager: None,
            virtual_keyboard: None,

            seat: None,
            keyboard_state: None,
            pointer: None,
            touch: None,

            namespace: "".to_owned(),
            keyboard_interactivity: zwlr_layer_surface_v1::KeyboardInteractivity::OnDemand,
            layer: Layer::Overlay,
            anchor: Anchor::Top | Anchor::Left | Anchor::Right | Anchor::Bottom,
            size: None,
            exclusive_zone: None,
            margin: None,

            use_display_handle: false,
            loop_handler: None,

            last_wloutput: None,
            last_unit_index: 0,

            return_data: Vec::new(),
        }
    }
}

impl<T> WindowState<T> {
    fn get_id_list(&self) -> Vec<id::Id> {
        self.units.iter().map(|unit| unit.id).collect()
    }
    /// You can save the virtual_keyboard here
    pub fn set_virtual_keyboard(&mut self, keyboard: ZwpVirtualKeyboardV1) {
        self.virtual_keyboard = Some(keyboard);
    }

    /// get the saved virtual_keyboard
    pub fn get_virtual_keyboard(&self) -> Option<&ZwpVirtualKeyboardV1> {
        self.virtual_keyboard.as_ref()
    }

    /// with loop_handler you can do more thing
    pub fn get_loop_handler(&self) -> Option<&LoopHandle<'static, Self>> {
        self.loop_handler.as_ref()
    }

    /// get the unit with the index returned by eventloop
    #[deprecated]
    pub fn get_unit(&mut self, index: usize) -> &mut WindowStateUnit<T> {
        &mut self.units[index]
    }

    /// remove it later
    #[deprecated]
    pub fn get_unit_option(&mut self, index: usize) -> Option<&mut WindowStateUnit<T>> {
        if index >= self.units.len() {
            None
        } else {
            Some(&mut self.units[index])
        }
    }

    pub fn get_mut_unit_with_id(&mut self, id: id::Id) -> Option<&mut WindowStateUnit<T>> {
        self.units.iter_mut().find(|unit| unit.id == id)
    }

    pub fn get_unit_with_id(&self, id: id::Id) -> Option<&WindowStateUnit<T>> {
        self.units.iter().find(|unit| unit.id == id)
    }

    /// it return the iter of units. you can do loop with it
    pub fn get_unit_iter(&self) -> impl Iterator<Item = &WindowStateUnit<T>> {
        self.units.iter()
    }

    /// it return the mut iter of units. you can do loop with it
    pub fn get_unit_iter_mut(&mut self) -> impl Iterator<Item = &mut WindowStateUnit<T>> {
        self.units.iter_mut()
    }

    fn surface_pos(&self) -> Option<usize> {
        self.units
            .iter()
            .position(|unit| Some(&unit.wl_surface) == self.current_surface.as_ref())
    }
    fn surface_id(&self) -> Option<id::Id> {
        self.units
            .iter()
            .find(|unit| Some(&unit.wl_surface) == self.current_surface.as_ref())
            .map(|unit| unit.id())
    }
    /// get the current focused surface id
    pub fn current_surface_id(&self) -> Option<id::Id> {
        self.units
            .iter()
            .find(|unit| Some(&unit.wl_surface) == self.current_surface.as_ref())
            .map(|unit| unit.id())
    }

    fn get_id_from_surface(&self, surface: &WlSurface) -> Option<id::Id> {
        self.units
            .iter()
            .find(|unit| &unit.wl_surface == surface)
            .map(|unit| unit.id())
    }
}

impl<T: 'static> Dispatch<wl_registry::WlRegistry, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        proxy: &wl_registry::WlRegistry,
        event: <wl_registry::WlRegistry as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qh: &wayland_client::QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                if interface == wl_output::WlOutput::interface().name {
                    let output = proxy.bind::<wl_output::WlOutput, _, _>(name, version, qh, ());
                    state.outputs.push((name, output.clone()));
                    state
                        .message
                        .push((None, DispatchMessageInner::NewDisplay(output)));
                }
            }
            wl_registry::Event::GlobalRemove { name } => {
                if state
                    .last_wloutput
                    .as_ref()
                    .is_some_and(|output| !output.is_alive())
                {
                    state.last_wloutput.take();
                }
                state.outputs.retain(|x| x.0 != name);
                state.units.retain(|unit| unit.wl_surface.is_alive());
            }

            _ => {}
        }
    }
}

impl<T: 'static> Dispatch<wl_seat::WlSeat, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: <wl_seat::WlSeat as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qh: &wayland_client::QueueHandle<Self>,
    ) {
        use xkb_keyboard::KeyboardState;
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        {
            if capabilities.contains(wl_seat::Capability::Keyboard) {
                state.keyboard_state = Some(KeyboardState::new(seat.get_keyboard(qh, ())));
            }
            if capabilities.contains(wl_seat::Capability::Pointer) {
                state.pointer = Some(seat.get_pointer(qh, ()));
            }
            if capabilities.contains(wl_seat::Capability::Touch) {
                state.touch = Some(seat.get_touch(qh, ()));
            }
        }
    }
}

impl<T> Dispatch<wl_keyboard::WlKeyboard, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        _proxy: &wl_keyboard::WlKeyboard,
        event: <wl_keyboard::WlKeyboard as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        use keyboard::*;
        use xkb_keyboard::ElementState;
        let keyboard_state = state.keyboard_state.as_mut().unwrap();
        match event {
            wl_keyboard::Event::Keymap { format, fd, size } => match format {
                WEnum::Value(KeymapFormat::XkbV1) => {
                    let context = &mut keyboard_state.xkb_context;
                    context.set_keymap_from_fd(fd, size as usize)
                }
                WEnum::Value(KeymapFormat::NoKeymap) => {
                    log::warn!("non-xkb compatible keymap")
                }
                _ => unreachable!(),
            },
            wl_keyboard::Event::Leave { .. } => {
                state.message.push((
                    state.surface_id(),
                    DispatchMessageInner::ModifiersChanged(ModifiersState::empty()),
                ));
            }
            wl_keyboard::Event::Key {
                state: keystate,
                key,
                ..
            } => {
                let pressed_state = match keystate {
                    WEnum::Value(KeyState::Pressed) => ElementState::Pressed,
                    WEnum::Value(KeyState::Released) => ElementState::Released,
                    _ => {
                        return;
                    }
                };
                let key = key + 8;
                if let Some(mut key_context) = keyboard_state.xkb_context.key_context() {
                    let event = key_context.process_key_event(key, pressed_state, false);
                    let event = DispatchMessageInner::KeyboardInput {
                        event,
                        is_synthetic: false,
                    };
                    state.message.push((state.surface_id(), event));
                }
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_locked,
                mods_latched,
                group,
                ..
            } => {
                let xkb_context = &mut keyboard_state.xkb_context;
                let xkb_state = match xkb_context.state_mut() {
                    Some(state) => state,
                    None => return,
                };
                xkb_state.update_modifiers(mods_depressed, mods_latched, mods_locked, 0, 0, group);
                let modifiers = xkb_state.modifiers();

                state.message.push((
                    state.surface_id(),
                    DispatchMessageInner::ModifiersChanged(modifiers.into()),
                ))
            }
            _ => {}
        }
    }
}

impl<T> Dispatch<wl_touch::WlTouch, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        _proxy: &wl_touch::WlTouch,
        event: <wl_touch::WlTouch as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_touch::Event::Down {
                serial,
                time,
                surface,
                id,
                x,
                y,
            } => state.message.push((
                state.get_id_from_surface(&surface),
                DispatchMessageInner::TouchDown {
                    serial,
                    time,
                    id,
                    x,
                    y,
                },
            )),
            wl_touch::Event::Up { serial, time, id } => state
                .message
                .push((None, DispatchMessageInner::TouchUp { serial, time, id })),
            wl_touch::Event::Motion { time, id, x, y } => state
                .message
                .push((None, DispatchMessageInner::TouchMotion { time, id, x, y })),
            _ => {}
        }
    }
}

impl<T> Dispatch<wl_pointer::WlPointer, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        pointer: &wl_pointer::WlPointer,
        event: <wl_pointer::WlPointer as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Axis { time, axis, value } => match axis {
                WEnum::Value(axis) => {
                    let (mut horizontal, mut vertical) = <(AxisScroll, AxisScroll)>::default();
                    match axis {
                        wl_pointer::Axis::VerticalScroll => {
                            vertical.absolute = value;
                        }
                        wl_pointer::Axis::HorizontalScroll => {
                            horizontal.absolute = value;
                        }
                        _ => unreachable!(),
                    };

                    state.message.push((
                        state.surface_id(),
                        DispatchMessageInner::Axis {
                            time,
                            horizontal,
                            vertical,
                            source: None,
                        },
                    ))
                }
                WEnum::Unknown(unknown) => {
                    log::warn!(target: "layershellev", "{}: invalid pointer axis: {:x}", pointer.id(), unknown);
                }
            },
            wl_pointer::Event::AxisStop { time, axis } => match axis {
                WEnum::Value(axis) => {
                    let (mut horizontal, mut vertical) = <(AxisScroll, AxisScroll)>::default();
                    match axis {
                        wl_pointer::Axis::VerticalScroll => vertical.stop = true,
                        wl_pointer::Axis::HorizontalScroll => horizontal.stop = true,

                        _ => unreachable!(),
                    }

                    state.message.push((
                        state.surface_id(),
                        DispatchMessageInner::Axis {
                            time,
                            horizontal,
                            vertical,
                            source: None,
                        },
                    ));
                }

                WEnum::Unknown(unknown) => {
                    log::warn!(target: "layershellev", "{}: invalid pointer axis: {:x}", pointer.id(), unknown);
                }
            },
            wl_pointer::Event::AxisSource { axis_source } => match axis_source {
                WEnum::Value(source) => state.message.push((
                    state.surface_id(),
                    DispatchMessageInner::Axis {
                        horizontal: AxisScroll::default(),
                        vertical: AxisScroll::default(),
                        source: Some(source),
                        time: 0,
                    },
                )),
                WEnum::Unknown(unknown) => {
                    log::warn!(target: "layershellev", "unknown pointer axis source: {:x}", unknown);
                }
            },
            wl_pointer::Event::AxisDiscrete { axis, discrete } => match axis {
                WEnum::Value(axis) => {
                    let (mut horizontal, mut vertical) = <(AxisScroll, AxisScroll)>::default();
                    match axis {
                        wl_pointer::Axis::VerticalScroll => {
                            vertical.discrete = discrete;
                        }

                        wl_pointer::Axis::HorizontalScroll => {
                            horizontal.discrete = discrete;
                        }

                        _ => unreachable!(),
                    };

                    state.message.push((
                        state.surface_id(),
                        DispatchMessageInner::Axis {
                            time: 0,
                            horizontal,
                            vertical,
                            source: None,
                        },
                    ));
                }

                WEnum::Unknown(unknown) => {
                    log::warn!(target: "layershellev", "{}: invalid pointer axis: {:x}", pointer.id(), unknown);
                }
            },
            wl_pointer::Event::Button {
                state: btnstate,
                serial,
                button,
                time,
            } => {
                state.message.push((
                    state.surface_id(),
                    DispatchMessageInner::MouseButton {
                        state: btnstate,
                        serial,
                        button,
                        time,
                    },
                ));
            }
            wl_pointer::Event::Leave { .. } => {
                state.current_surface = None;
                state
                    .message
                    .push((state.surface_id(), DispatchMessageInner::MouseLeave));
            }
            wl_pointer::Event::Enter {
                serial,
                surface,
                surface_x,
                surface_y,
            } => {
                state.current_surface = Some(surface.clone());
                let surface_id = state.surface_id();

                if let Some(unit) = surface_id.and_then(|id| state.get_unit_with_id(id)) {
                    state.last_unit_index = state
                        .outputs
                        .iter()
                        .position(|(_, output)| {
                            unit.wl_output
                                .as_ref()
                                .is_some_and(|uoutput| uoutput == output)
                        })
                        .unwrap_or(0);
                }

                state.message.push((
                    surface_id,
                    DispatchMessageInner::MouseEnter {
                        pointer: pointer.clone(),
                        serial,
                        surface_x,
                        surface_y,
                    },
                ));
            }
            wl_pointer::Event::Motion {
                time,
                surface_x,
                surface_y,
            } => {
                state.message.push((
                    state.surface_id(),
                    DispatchMessageInner::MouseMotion {
                        time,
                        surface_x,
                        surface_y,
                    },
                ));
            }
            _ => {
                // TODO: not now
            }
        }
    }
}

impl<T> Dispatch<xdg_surface::XdgSurface, ()> for WindowState<T> {
    fn event(
        _state: &mut Self,
        surface: &xdg_surface::XdgSurface,
        event: <xdg_surface::XdgSurface as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            surface.ack_configure(serial);
        }
    }
}
impl<T> Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        surface: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: <zwlr_layer_surface_v1::ZwlrLayerSurfaceV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
        if let zwlr_layer_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = event
        {
            surface.ack_configure(serial);

            let Some(unit_index) = state.units.iter().position(|unit| unit.shell == *surface)
            else {
                return;
            };
            state.units[unit_index].size = (width, height);

            state.message.push((
                Some(state.units[unit_index].id),
                DispatchMessageInner::RefreshSurface { width, height },
            ));
        }
    }
}

impl<T> Dispatch<xdg_popup::XdgPopup, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        surface: &xdg_popup::XdgPopup,
        event: <xdg_popup::XdgPopup as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if let xdg_popup::Event::Configure { width, height, .. } = event {
            let Some(unit_index) = state.units.iter().position(|unit| unit.shell == *surface)
            else {
                return;
            };
            let id = state.units[unit_index].id;
            state.units[unit_index].size = (width as u32, height as u32);

            state.message.push((
                Some(id),
                DispatchMessageInner::RefreshSurface {
                    width: width as u32,
                    height: height as u32,
                },
            ));
        }
    }
}

impl<T> Dispatch<zxdg_output_v1::ZxdgOutputV1, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        proxy: &zxdg_output_v1::ZxdgOutputV1,
        event: <zxdg_output_v1::ZxdgOutputV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let Some(index) = state.units.iter().position(|info| {
            info.zxdgoutput
                .as_ref()
                .is_some_and(|zxdgoutput| zxdgoutput.zxdgoutput == *proxy)
        }) else {
            return;
        };
        let info = &mut state.units[index];
        let xdg_info = info.zxdgoutput.as_mut().unwrap();
        let change_type = match event {
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                xdg_info.logical_size = (width, height);
                XdgInfoChangedType::Size
            }
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                xdg_info.position = (x, y);
                XdgInfoChangedType::Position
            }
            _ => {
                return;
            }
        };
        state.message.push((
            Some(state.units[index].id),
            DispatchMessageInner::XdgInfoChanged(change_type),
        ));
    }
}

impl<T> Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        proxy: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: <wp_fractional_scale_v1::WpFractionalScaleV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            let Some(id) = state
                .units
                .iter()
                .find(|info| {
                    info.fractional_scale
                        .as_ref()
                        .is_some_and(|fractional_scale| fractional_scale == proxy)
                })
                .map(|unit| unit.id)
            else {
                return;
            };
            state
                .message
                .push((Some(id), DispatchMessageInner::PrefredScale(scale)));
        }
    }
}

delegate_noop!(@<T> WindowState<T>: ignore WlCompositor); // WlCompositor is need to create a surface
delegate_noop!(@<T> WindowState<T>: ignore WlSurface); // surface is the base needed to show buffer
delegate_noop!(@<T> WindowState<T>: ignore WlOutput); // output is need to place layer_shell, although here
                                                      // it is not used
delegate_noop!(@<T> WindowState<T>: ignore WlShm); // shm is used to create buffer pool
delegate_noop!(@<T> WindowState<T>: ignore WlShmPool); // so it is pool, created by wl_shm
delegate_noop!(@<T> WindowState<T>: ignore WlBuffer); // buffer show the picture
delegate_noop!(@<T> WindowState<T>: ignore ZwlrLayerShellV1); // it is similar with xdg_toplevel, also the
                                                              // ext-session-shell

delegate_noop!(@<T> WindowState<T>: ignore WpCursorShapeManagerV1);
delegate_noop!(@<T> WindowState<T>: ignore WpCursorShapeDeviceV1);

delegate_noop!(@<T> WindowState<T>: ignore ZwpVirtualKeyboardV1);
delegate_noop!(@<T> WindowState<T>: ignore ZwpVirtualKeyboardManagerV1);

delegate_noop!(@<T> WindowState<T>: ignore ZxdgOutputManagerV1);
delegate_noop!(@<T> WindowState<T>: ignore WpFractionalScaleManagerV1);
delegate_noop!(@<T> WindowState<T>: ignore XdgPositioner);
delegate_noop!(@<T> WindowState<T>: ignore XdgWmBase);

impl<T: 'static> WindowState<T> {
    /// build a new WindowState
    pub fn build(mut self) -> Result<Self, LayerEventError> {
        let connection = Connection::connect_to_env()?;
        let (globals, _) = registry_queue_init::<BaseState>(&connection)?; // We just need the
                                                                           // global, the
                                                                           // event_queue is
                                                                           // not needed, we
                                                                           // do not need
                                                                           // BaseState after
                                                                           // this anymore

        let mut event_queue = connection.new_event_queue::<WindowState<T>>();
        let qh = event_queue.handle();

        let wmcompositer = globals.bind::<WlCompositor, _, _>(&qh, 1..=5, ())?; // so the first
                                                                                // thing is to
                                                                                // get WlCompositor

        // we need to create more

        let shm = globals.bind::<WlShm, _, _>(&qh, 1..=1, ())?;
        self.shm = Some(shm);
        self.seat = Some(globals.bind::<WlSeat, _, _>(&qh, 1..=1, ())?);

        let wmbase = globals.bind::<XdgWmBase, _, _>(&qh, 2..=6, ())?;
        self.wmbase = Some(wmbase);

        let cursor_manager = globals
            .bind::<WpCursorShapeManagerV1, _, _>(&qh, 1..=1, ())
            .ok();

        let _ = connection.display().get_registry(&qh, ()); // so if you want WlOutput, you need to
                                                            // register this

        let xdg_output_manager = globals.bind::<ZxdgOutputManagerV1, _, _>(&qh, 1..=3, ())?; // bind
                                                                                             // xdg_output_manager

        let fractional_scale_manager = globals
            .bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ())
            .ok();

        event_queue.blocking_dispatch(&mut self)?; // then make a dispatch

        // do the step before, you get empty list

        // so it is the same way, to get surface detach to protocol, first get the shell, like wmbase
        // or layer_shell or session-shell, then get `surface` from the wl_surface you get before, and
        // set it
        // finally thing to remember is to commit the surface, make the shell to init.
        //let (init_w, init_h) = self.size;
        // this example is ok for both xdg_surface and layer_shell
        if self.is_single {
            let wl_surface = wmcompositer.create_surface(&qh, ()); // and create a surface. if two or more,
            let layer_shell = globals
                .bind::<ZwlrLayerShellV1, _, _>(&qh, 3..=4, ())
                .unwrap();
            let layer = layer_shell.get_layer_surface(
                &wl_surface,
                None,
                self.layer,
                self.namespace.clone(),
                &qh,
                (),
            );
            layer.set_anchor(self.anchor);
            layer.set_keyboard_interactivity(self.keyboard_interactivity);
            if let Some((init_w, init_h)) = self.size {
                layer.set_size(init_w, init_h);
            }

            if let Some(zone) = self.exclusive_zone {
                layer.set_exclusive_zone(zone);
            }

            if let Some((top, right, bottom, left)) = self.margin {
                layer.set_margin(top, right, bottom, left);
            }

            wl_surface.commit();

            let mut fractional_scale = None;
            if let Some(ref fractional_scale_manager) = fractional_scale_manager {
                fractional_scale =
                    Some(fractional_scale_manager.get_fractional_scale(&wl_surface, &qh, ()));
            }
            // so during the init Configure of the shell, a buffer, atleast a buffer is needed.
            // and if you need to reconfigure it, you need to commit the wl_surface again
            // so because this is just an example, so we just commit it once
            // like if you want to reset anchor or KeyboardInteractivity or resize, commit is needed
            self.units.push(WindowStateUnit {
                id: id::Id::unique(),
                display: connection.display(),
                wl_surface,
                size: (0, 0),
                buffer: None,
                shell: Shell::LayerShell(layer),
                zxdgoutput: None,
                fractional_scale,
                binding: None,
                becreated: false,
                wl_output: None,
            });
        } else {
            let displays = self.outputs.clone();
            for (_, output_display) in displays.iter() {
                let wl_surface = wmcompositer.create_surface(&qh, ()); // and create a surface. if two or more,
                let layer_shell = globals
                    .bind::<ZwlrLayerShellV1, _, _>(&qh, 3..=4, ())
                    .unwrap();
                let layer = layer_shell.get_layer_surface(
                    &wl_surface,
                    Some(output_display),
                    self.layer,
                    self.namespace.clone(),
                    &qh,
                    (),
                );
                layer.set_anchor(self.anchor);
                layer.set_keyboard_interactivity(self.keyboard_interactivity);
                if let Some((init_w, init_h)) = self.size {
                    layer.set_size(init_w, init_h);
                }

                if let Some(zone) = self.exclusive_zone {
                    layer.set_exclusive_zone(zone);
                }

                if let Some((top, right, bottom, left)) = self.margin {
                    layer.set_margin(top, right, bottom, left);
                }

                wl_surface.commit();

                let zxdgoutput = xdg_output_manager.get_xdg_output(output_display, &qh, ());
                let mut fractional_scale = None;
                if let Some(ref fractional_scale_manager) = fractional_scale_manager {
                    fractional_scale =
                        Some(fractional_scale_manager.get_fractional_scale(&wl_surface, &qh, ()));
                }
                // so during the init Configure of the shell, a buffer, atleast a buffer is needed.
                // and if you need to reconfigure it, you need to commit the wl_surface again
                // so because this is just an example, so we just commit it once
                // like if you want to reset anchor or KeyboardInteractivity or resize, commit is needed

                self.units.push(WindowStateUnit {
                    id: id::Id::unique(),
                    display: connection.display(),
                    wl_surface,
                    size: (0, 0),
                    buffer: None,
                    shell: Shell::LayerShell(layer),
                    zxdgoutput: Some(ZxdgOutputInfo::new(zxdgoutput)),
                    fractional_scale,
                    binding: None,
                    becreated: false,
                    wl_output: Some(output_display.clone()),
                });
            }
            self.message.clear();
        }
        self.event_queue = Some(event_queue);
        self.globals = Some(globals);
        self.wl_compositor = Some(wmcompositer);
        self.fractional_scale_manager = fractional_scale_manager;
        self.cursor_manager = cursor_manager;
        self.xdg_output_manager = Some(xdg_output_manager);
        self.connection = Some(connection);

        Ok(self)
    }
    /// main event loop, every time dispatch, it will store the messages, and do callback. it will
    /// pass a LayerEvent, with self as mut, the last `Option<usize>` describe which unit the event
    /// happened on, like tell you this time you do a click, what surface it is on. you can use the
    /// index to get the unit, with [WindowState::get_unit] if the even is not spical on one surface,
    /// it will return [None].
    /// main event loop, every time dispatch, it will store the messages, and do callback. it will
    /// pass a LayerEvent, with self as mut, the last `Option<usize>` describe which unit the event
    /// happened on, like tell you this time you do a click, what surface it is on. you can use the
    /// index to get the unit, with [WindowState::get_unit] if the even is not spical on one surface,
    /// it will return [None].
    ///
    /// Different with running, it receiver a receiver
    pub fn running_with_proxy<F, Message>(
        self,
        message_receiver: std::sync::mpsc::Receiver<Message>,
        event_handler: F,
    ) -> Result<(), LayerEventError>
    where
        F: FnMut(LayerEvent<T, Message>, &mut WindowState<T>, Option<id::Id>) -> ReturnData<T>,
    {
        self.running_with_proxy_option(Some(message_receiver), event_handler)
    }
    /// main event loop, every time dispatch, it will store the messages, and do callback. it will
    /// pass a LayerEvent, with self as mut, the last `Option<usize>` describe which unit the event
    /// happened on, like tell you this time you do a click, what surface it is on. you can use the
    /// index to get the unit, with [WindowState::get_unit] if the even is not spical on one surface,
    /// it will return [None].
    /// main event loop, every time dispatch, it will store the messages, and do callback. it will
    /// pass a LayerEvent, with self as mut, the last `Option<usize>` describe which unit the event
    /// happened on, like tell you this time you do a click, what surface it is on. you can use the
    /// index to get the unit, with [WindowState::get_unit] if the even is not spical on one surface,
    /// it will return [None].
    ///
    ///
    pub fn running<F>(self, event_handler: F) -> Result<(), LayerEventError>
    where
        F: FnMut(LayerEvent<T, ()>, &mut WindowState<T>, Option<id::Id>) -> ReturnData<T>,
    {
        self.running_with_proxy_option(None, event_handler)
    }

    fn running_with_proxy_option<F, Message>(
        mut self,
        message_receiver: Option<std::sync::mpsc::Receiver<Message>>,
        mut event_handler: F,
    ) -> Result<(), LayerEventError>
    where
        F: FnMut(LayerEvent<T, Message>, &mut WindowState<T>, Option<id::Id>) -> ReturnData<T>,
    {
        let globals = self.globals.take().unwrap();
        let event_queue = self.event_queue.take().unwrap();
        let qh = event_queue.handle();
        let wmcompositer = self.wl_compositor.take().unwrap();
        let shm = self.shm.take().unwrap();
        let fractional_scale_manager = self.fractional_scale_manager.take();
        let cursor_manager: Option<WpCursorShapeManagerV1> = self.cursor_manager.take();
        let xdg_output_manager = self.xdg_output_manager.take().unwrap();
        let connection = self.connection.take().unwrap();
        let mut init_event = None;
        let wmbase = self.wmbase.take().unwrap();

        while !matches!(init_event, Some(ReturnData::None)) {
            match init_event {
                None => {
                    init_event = Some(event_handler(LayerEvent::InitRequest, &mut self, None));
                }
                Some(ReturnData::RequestBind) => {
                    init_event = Some(event_handler(
                        LayerEvent::BindProvide(&globals, &qh),
                        &mut self,
                        None,
                    ));
                }
                _ => panic!("Not provide server here"),
            }
        }

        let mut event_loop: EventLoop<Self> =
            EventLoop::try_new().expect("Failed to initialize the event loop");

        WaylandSource::new(connection.clone(), event_queue)
            .insert(event_loop.handle())
            .expect("Failed to init wayland source");

        self.loop_handler = Some(event_loop.handle());

        'out: loop {
            event_loop.dispatch(Duration::from_millis(1), &mut self)?;

            let mut messages = Vec::new();
            std::mem::swap(&mut messages, &mut self.message);
            for msg in messages.iter() {
                match msg {
                    (Some(unit_index), DispatchMessageInner::RefreshSurface { width, height }) => {
                        let Some(index) = self.units.iter().position(|unit| unit.id == *unit_index)
                        else {
                            continue;
                        };
                        if self.units[index].buffer.is_none() && !self.use_display_handle {
                            let mut file = tempfile::tempfile()?;
                            let ReturnData::WlBuffer(buffer) = event_handler(
                                LayerEvent::RequestBuffer(&mut file, &shm, &qh, *width, *height),
                                &mut self,
                                Some(*unit_index),
                            ) else {
                                panic!("You cannot return this one");
                            };
                            let surface = &self.units[index].wl_surface;
                            surface.attach(Some(&buffer), 0, 0);
                            self.units[index].buffer = Some(buffer);
                        } else {
                            event_handler(
                                LayerEvent::RequestMessages(&DispatchMessage::RequestRefresh {
                                    width: *width,
                                    height: *height,
                                    is_created: self.units[index].becreated,
                                }),
                                &mut self,
                                Some(*unit_index),
                            );
                        }

                        if let Some(unit) = self.get_unit_with_id(*unit_index) {
                            unit.wl_surface.commit();
                        }
                    }
                    (index_info, DispatchMessageInner::XdgInfoChanged(change_type)) => {
                        event_handler(
                            LayerEvent::XdgInfoChanged(*change_type),
                            &mut self,
                            *index_info,
                        );
                    }
                    (_, DispatchMessageInner::NewDisplay(output_display)) => {
                        if self.is_single {
                            continue;
                        }
                        let wl_surface = wmcompositer.create_surface(&qh, ()); // and create a surface. if two or more,
                        let layer_shell = globals
                            .bind::<ZwlrLayerShellV1, _, _>(&qh, 3..=4, ())
                            .unwrap();
                        let layer = layer_shell.get_layer_surface(
                            &wl_surface,
                            Some(output_display),
                            self.layer,
                            self.namespace.clone(),
                            &qh,
                            (),
                        );
                        layer.set_anchor(self.anchor);
                        layer.set_keyboard_interactivity(self.keyboard_interactivity);
                        if let Some((init_w, init_h)) = self.size {
                            layer.set_size(init_w, init_h);
                        }

                        if let Some(zone) = self.exclusive_zone {
                            layer.set_exclusive_zone(zone);
                        }

                        if let Some((top, right, bottom, left)) = self.margin {
                            layer.set_margin(top, right, bottom, left);
                        }

                        wl_surface.commit();

                        let zxdgoutput = xdg_output_manager.get_xdg_output(output_display, &qh, ());
                        let mut fractional_scale = None;
                        if let Some(ref fractional_scale_manager) = fractional_scale_manager {
                            fractional_scale = Some(fractional_scale_manager.get_fractional_scale(
                                &wl_surface,
                                &qh,
                                (),
                            ));
                        }
                        // so during the init Configure of the shell, a buffer, atleast a buffer is needed.
                        // and if you need to reconfigure it, you need to commit the wl_surface again
                        // so because this is just an example, so we just commit it once
                        // like if you want to reset anchor or KeyboardInteractivity or resize, commit is needed

                        self.units.push(WindowStateUnit {
                            id: id::Id::unique(),
                            display: connection.display(),
                            wl_surface,
                            size: (0, 0),
                            buffer: None,
                            shell: Shell::LayerShell(layer),
                            zxdgoutput: Some(ZxdgOutputInfo::new(zxdgoutput)),
                            fractional_scale,
                            binding: None,
                            becreated: false,
                            wl_output: Some(output_display.clone()),
                        });
                    }
                    _ => {
                        let (index_message, msg) = msg;

                        let msg: DispatchMessage = msg.clone().into();
                        match event_handler(
                            LayerEvent::RequestMessages(&msg),
                            &mut self,
                            *index_message,
                        ) {
                            ReturnData::RedrawAllRequest => {
                                let idlist = self.get_id_list();
                                for id in idlist {
                                    if let Some(unit) = self.get_unit_with_id(id) {
                                        if unit.size.0 == 0 || unit.size.1 == 0 {
                                            continue;
                                        }
                                        event_handler(
                                            LayerEvent::RequestMessages(
                                                &DispatchMessage::RequestRefresh {
                                                    width: unit.size.0,
                                                    height: unit.size.1,
                                                    is_created: unit.becreated,
                                                },
                                            ),
                                            &mut self,
                                            Some(id),
                                        );
                                    }
                                }
                            }
                            ReturnData::RedrawIndexRequest(id) => {
                                if let Some(unit) = self.get_unit_with_id(id) {
                                    event_handler(
                                        LayerEvent::RequestMessages(
                                            &DispatchMessage::RequestRefresh {
                                                width: unit.size.0,
                                                height: unit.size.1,
                                                is_created: unit.becreated,
                                            },
                                        ),
                                        &mut self,
                                        Some(id),
                                    );
                                }
                            }
                            ReturnData::RequestExist => {
                                break 'out;
                            }
                            ReturnData::RequestSetCursorShape((shape_name, pointer, serial)) => {
                                if let Some(ref cursor_manager) = cursor_manager {
                                    let Some(shape) = str_to_shape(&shape_name) else {
                                        log::error!("Not supported shape");
                                        continue;
                                    };
                                    let device = cursor_manager.get_pointer(&pointer, &qh, ());
                                    device.set_shape(serial, shape);
                                    device.destroy();
                                } else {
                                    let Some(cursor_buffer) =
                                        get_cursor_buffer(&shape_name, &connection, &shm)
                                    else {
                                        log::error!("Cannot find cursor {shape_name}");
                                        continue;
                                    };
                                    let cursor_surface = wmcompositer.create_surface(&qh, ());
                                    cursor_surface.attach(Some(&cursor_buffer), 0, 0);
                                    // and create a surface. if two or more,
                                    let (hotspot_x, hotspot_y) = cursor_buffer.hotspot();
                                    pointer.set_cursor(
                                        serial,
                                        Some(&cursor_surface),
                                        hotspot_x as i32,
                                        hotspot_y as i32,
                                    );
                                    cursor_surface.commit();
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            if let Some(event) = message_receiver.as_ref().and_then(|rv| rv.try_recv().ok()) {
                match event_handler(LayerEvent::UserEvent(event), &mut self, None) {
                    ReturnData::RequestExist => {
                        break 'out;
                    }
                    ReturnData::RequestSetCursorShape((shape_name, pointer, serial)) => {
                        if let Some(ref cursor_manager) = cursor_manager {
                            let Some(shape) = str_to_shape(&shape_name) else {
                                log::error!("Not supported shape");
                                continue;
                            };
                            let device = cursor_manager.get_pointer(&pointer, &qh, ());
                            device.set_shape(serial, shape);
                            device.destroy();
                        } else {
                            let Some(cursor_buffer) =
                                get_cursor_buffer(&shape_name, &connection, &shm)
                            else {
                                log::error!("Cannot find cursor {shape_name}");
                                continue;
                            };
                            let cursor_surface = wmcompositer.create_surface(&qh, ());
                            cursor_surface.attach(Some(&cursor_buffer), 0, 0);
                            // and create a surface. if two or more,
                            let (hotspot_x, hotspot_y) = cursor_buffer.hotspot();
                            pointer.set_cursor(
                                serial,
                                Some(&cursor_surface),
                                hotspot_x as i32,
                                hotspot_y as i32,
                            );
                            cursor_surface.commit();
                        }
                    }
                    _ => {}
                }
            }
            let mut return_data = vec![event_handler(LayerEvent::NormalDispatch, &mut self, None)];
            loop {
                return_data.append(&mut self.return_data);

                let mut replace_data = Vec::new();
                for data in return_data {
                    match data {
                        ReturnData::RedrawAllRequest => {
                            let idlist = self.get_id_list();
                            for id in idlist {
                                if let Some(unit) = self.get_unit_with_id(id) {
                                    if unit.size.0 == 0 || unit.size.1 == 0 {
                                        continue;
                                    }
                                    event_handler(
                                        LayerEvent::RequestMessages(
                                            &DispatchMessage::RequestRefresh {
                                                width: unit.size.0,
                                                height: unit.size.1,
                                                is_created: unit.becreated,
                                            },
                                        ),
                                        &mut self,
                                        Some(id),
                                    );
                                }
                            }
                        }
                        ReturnData::RedrawIndexRequest(id) => {
                            if let Some(unit) = self.get_unit_with_id(id) {
                                replace_data.push(event_handler(
                                    LayerEvent::RequestMessages(&DispatchMessage::RequestRefresh {
                                        width: unit.size.0,
                                        height: unit.size.1,
                                        is_created: unit.becreated,
                                    }),
                                    &mut self,
                                    Some(id),
                                ));
                            }
                        }
                        ReturnData::RequestExist => {
                            break 'out;
                        }
                        ReturnData::RequestSetCursorShape((shape_name, pointer, serial)) => {
                            if let Some(ref cursor_manager) = cursor_manager {
                                let Some(shape) = str_to_shape(&shape_name) else {
                                    log::error!("Not supported shape");
                                    continue;
                                };
                                let device = cursor_manager.get_pointer(&pointer, &qh, ());
                                device.set_shape(serial, shape);
                                device.destroy();
                            } else {
                                let Some(cursor_buffer) =
                                    get_cursor_buffer(&shape_name, &connection, &shm)
                                else {
                                    log::error!("Cannot find cursor {shape_name}");
                                    continue;
                                };
                                let cursor_surface = wmcompositer.create_surface(&qh, ());
                                cursor_surface.attach(Some(&cursor_buffer), 0, 0);
                                // and create a surface. if two or more,
                                let (hotspot_x, hotspot_y) = cursor_buffer.hotspot();
                                pointer.set_cursor(
                                    serial,
                                    Some(&cursor_surface),
                                    hotspot_x as i32,
                                    hotspot_y as i32,
                                );
                                cursor_surface.commit();
                            }
                        }
                        ReturnData::NewLayerShell((
                            NewLayerShellSettings {
                                size,
                                layer,
                                anchor,
                                exclusive_zone,
                                margin,
                                keyboard_interactivity,
                                use_last_output,
                            },
                            info,
                        )) => {
                            if self.is_single {
                                continue;
                            }
                            let pos = self.surface_pos();

                            let mut output = pos.and_then(|p| self.units[p].wl_output.as_ref());

                            if self.last_wloutput.is_none()
                                && self.outputs.len() > self.last_unit_index
                            {
                                self.last_wloutput =
                                    Some(self.outputs[self.last_unit_index].1.clone());
                            }

                            if use_last_output {
                                output = self.last_wloutput.as_ref();
                            }

                            let wl_surface = wmcompositer.create_surface(&qh, ()); // and create a surface. if two or more,
                            let layer_shell = globals
                                .bind::<ZwlrLayerShellV1, _, _>(&qh, 3..=4, ())
                                .unwrap();
                            let layer = layer_shell.get_layer_surface(
                                &wl_surface,
                                output,
                                layer,
                                self.namespace.clone(),
                                &qh,
                                (),
                            );
                            layer.set_anchor(anchor);
                            layer.set_keyboard_interactivity(keyboard_interactivity);
                            if let Some((init_w, init_h)) = size {
                                layer.set_size(init_w, init_h);
                            }

                            if let Some(zone) = exclusive_zone {
                                layer.set_exclusive_zone(zone);
                            }

                            if let Some((top, right, bottom, left)) = margin {
                                layer.set_margin(top, right, bottom, left);
                            }

                            wl_surface.commit();

                            let mut fractional_scale = None;
                            if let Some(ref fractional_scale_manager) = fractional_scale_manager {
                                fractional_scale =
                                    Some(fractional_scale_manager.get_fractional_scale(
                                        &wl_surface,
                                        &qh,
                                        (),
                                    ));
                            }
                            // so during the init Configure of the shell, a buffer, atleast a buffer is needed.
                            // and if you need to reconfigure it, you need to commit the wl_surface again
                            // so because this is just an example, so we just commit it once
                            // like if you want to reset anchor or KeyboardInteractivity or resize, commit is needed

                            self.units.push(WindowStateUnit {
                                id: id::Id::unique(),
                                display: connection.display(),
                                wl_surface,
                                size: (0, 0),
                                buffer: None,
                                shell: Shell::LayerShell(layer),
                                zxdgoutput: None,
                                fractional_scale,
                                becreated: true,
                                wl_output: output.cloned(),
                                binding: info,
                            });
                        }
                        ReturnData::NewPopUp((
                            NewPopUpSettings {
                                size: (width, height),
                                position: (x, y),
                                id,
                            },
                            info,
                        )) => {
                            let Some(index) = self
                                .units
                                .iter()
                                .position(|unit| !unit.is_popup() && unit.id == id)
                            else {
                                continue;
                            };
                            let wl_surface = wmcompositer.create_surface(&qh, ());
                            let positioner = wmbase.create_positioner(&qh, ());
                            positioner.set_size(width as i32, height as i32);
                            positioner.set_anchor_rect(x, y, width as i32, height as i32);
                            let wl_xdg_surface = wmbase.get_xdg_surface(&wl_surface, &qh, ());
                            let popup = wl_xdg_surface.get_popup(None, &positioner, &qh, ());

                            let Shell::LayerShell(shell) = &self.units[index].shell else {
                                unreachable!()
                            };
                            shell.get_popup(&popup);

                            let mut fractional_scale = None;
                            if let Some(ref fractional_scale_manager) = fractional_scale_manager {
                                fractional_scale =
                                    Some(fractional_scale_manager.get_fractional_scale(
                                        &wl_surface,
                                        &qh,
                                        (),
                                    ));
                            }
                            wl_surface.commit();

                            self.units.push(WindowStateUnit {
                                id: id::Id::unique(),
                                display: connection.display(),
                                wl_surface,
                                size: (width, height),
                                buffer: None,
                                shell: Shell::PopUp((popup, wl_xdg_surface)),
                                zxdgoutput: None,
                                fractional_scale,
                                becreated: true,
                                wl_output: None,
                                binding: info,
                            });
                        }
                        _ => {}
                    }
                }
                replace_data.retain(|x| !matches!(x, ReturnData::None));
                if replace_data.is_empty() {
                    break;
                }
                return_data = replace_data;
            }
            continue;
        }
        Ok(())
    }
}

fn get_cursor_buffer(
    shape: &str,
    connection: &Connection,
    shm: &WlShm,
) -> Option<CursorImageBuffer> {
    let mut cursor_theme = CursorTheme::load(connection, shm.clone(), 23).ok()?;
    let cursor = cursor_theme.get_cursor(shape);
    Some(cursor?[0].clone())
}
