use std::process;
use std::sync::Mutex;

use console::style;
use vnt::{ConnectInfo, ErrorInfo, ErrorType, HandshakeInfo, RegisterInfo, VntCallback};

#[derive(Clone)]
pub struct VntHandler {}

struct CallbackPrintState {
    last_handshake: Option<String>,
    last_register: Option<String>,
}

static PRINT_STATE: Mutex<CallbackPrintState> = Mutex::new(CallbackPrintState {
    last_handshake: None,
    last_register: None,
});

impl VntCallback for VntHandler {
    fn success(&self) {
        println!(" {} ", style("====== Connect Successfully ======").green())
    }
    #[cfg(feature = "integrated_tun")]
    fn create_tun(&self, info: vnt::DeviceInfo) {
        println!("create_tun {}", info)
    }

    fn connect(&self, info: ConnectInfo) {
        println!("connect {}", info)
    }

    fn handshake(&self, info: HandshakeInfo) -> bool {
        let text = info.to_string();
        let mut state = PRINT_STATE.lock().unwrap();
        if state.last_handshake.as_deref() != Some(text.as_str()) {
            println!("handshake {}", text);
            state.last_handshake = Some(text);
        }
        true
    }

    fn register(&self, info: RegisterInfo) -> bool {
        let text = info.to_string();
        let mut state = PRINT_STATE.lock().unwrap();
        if state.last_register.as_deref() != Some(text.as_str()) {
            println!("register {}", style(text.clone()).green());
            state.last_register = Some(text);
        }
        true
    }

    fn error(&self, info: ErrorInfo) {
        log::error!("error {:?}", info);
        println!("{}", style(format!("error {}", info)).red());
        match info.code {
            ErrorType::TokenError
            | ErrorType::AddressExhausted
            | ErrorType::IpAlreadyExists
            | ErrorType::InvalidIp
            | ErrorType::LocalIpExists
            | ErrorType::FailedToCrateDevice => {
                self.stop();
            }
            _ => {}
        }
    }

    fn stop(&self) {
        println!("stopped");
        process::exit(0)
    }
}
