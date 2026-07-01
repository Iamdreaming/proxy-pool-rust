//! Generated protobuf modules from xray-core proto files.
//!
//! The module hierarchy mirrors the protobuf package paths so that the
//! generated code's `super` references resolve correctly.

pub mod xray {
    pub mod app {
        pub mod proxyman {
            pub mod command {
                tonic::include_proto!("xray.app.proxyman.command");
            }
        }
    }
    pub mod core {
        tonic::include_proto!("xray.core");
    }
    pub mod common {
        pub mod geodata {
            tonic::include_proto!("xray.common.geodata");
        }
        pub mod net {
            tonic::include_proto!("xray.common.net");
        }
        pub mod protocol {
            tonic::include_proto!("xray.common.protocol");
        }
        pub mod serial {
            tonic::include_proto!("xray.common.serial");
        }
    }
    pub mod proxy {
        pub mod freedom {
            tonic::include_proto!("xray.proxy.freedom");
        }
        pub mod shadowsocks {
            tonic::include_proto!("xray.proxy.shadowsocks");
        }
        pub mod socks {
            tonic::include_proto!("xray.proxy.socks");
        }
        pub mod trojan {
            tonic::include_proto!("xray.proxy.trojan");
        }
        pub mod vmess {
            pub mod outbound {
                tonic::include_proto!("xray.proxy.vmess.outbound");
            }
        }
    }
    pub mod transport {
        pub mod internet {
            tonic::include_proto!("xray.transport.internet");
        }
    }
}
