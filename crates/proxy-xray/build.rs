use std::io::Result;

fn main() -> Result<()> {
    let proto_root = "protos";

    let proto_files = [
        "xray/app/proxyman/command/command.proto",
        "xray/proxy/shadowsocks/config.proto",
        "xray/proxy/vmess/outbound/config.proto",
        "xray/proxy/trojan/config.proto",
        "xray/proxy/socks/config.proto",
        "xray/proxy/freedom/config.proto",
    ];

    tonic_build::configure()
        .build_server(false)
        .compile_protos(&proto_files, &[proto_root])?;

    Ok(())
}
