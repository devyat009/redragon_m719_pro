use hidapi::HidApi;

fn calc_packet_checksum(pkt: &[u8; 17]) -> u8 {
    let mut sum: u8 = 0;
    for i in 0..16 {
        sum = sum.wrapping_add(pkt[i]);
    }
    (0x55u8).wrapping_sub(sum)
}

fn main() {
    let api = HidApi::new().expect("Failed to initialize hidapi");
    println!("=== Enumerating 25A7 devices ===");
    let mut dev_02 = None;
    let mut dev_01 = None;
    let mut dev_04 = None;

    for dev in api.device_list() {
        if dev.vendor_id() == 0x25A7 {
            println!(
                "PID: 0x{:04X}, UsagePage: 0x{:04X}, Usage: 0x{:04X}, Path: {:?}",
                dev.product_id(),
                dev.usage_page(),
                dev.usage(),
                dev.path()
            );

            if dev.usage_page() == 0xFF02 && dev_02.is_none() {
                dev_02 = dev.open_device(&api).ok();
            }
            if dev.usage_page() == 0xFF01 && dev_01.is_none() {
                dev_01 = dev.open_device(&api).ok();
            }
            if dev.usage_page() == 0xFF04 && dev_04.is_none() {
                dev_04 = dev.open_device(&api).ok();
            }
        }
    }

    println!("\nDevice handles: 0xFF02: {:?}, 0xFF01: {:?}, 0xFF04: {:?}", dev_02.is_some(), dev_01.is_some(), dev_04.is_some());

    if let (Some(cmd), Some(notify)) = (dev_02, dev_01) {
        println!("\n--- Test 1: Send GetConnectStatus probe [0x08, 0x03, ...] ---");
        let mut pkt = [0u8; 17];
        pkt[0] = 0x08;
        pkt[1] = 0x03;
        pkt[16] = calc_packet_checksum(&pkt);
        println!("Sending feature report: {:02X?}", pkt);
        match cmd.send_feature_report(&pkt) {
            Ok(_) => println!("send_feature_report SUCCESS!"),
            Err(e) => println!("send_feature_report ERROR: {}", e),
        }

        println!("Reading from 0xFF01 with timeout 500ms...");
        let mut buf = [0u8; 64];
        match notify.read_timeout(&mut buf, 500) {
            Ok(n) => {
                println!("Read {} bytes from 0xFF01: {:02X?}", n, &buf[..n]);
            }
            Err(e) => println!("read_timeout ERROR: {}", e),
        }

        println!("\n--- Test 2: Try GetInputReport on 0xFF01 ---");
        let mut in_buf = [0u8; 64];
        in_buf[0] = 0x09;
        match notify.get_input_report(&mut in_buf) {
            Ok(n) => println!("get_input_report on 0xFF01 returned {} bytes: {:02X?}", n, &in_buf[..n]),
            Err(e) => println!("get_input_report error: {}", e),
        }
    }

    if let Some(dev) = dev_04 {
        println!("\n--- Test 3: GetFeatureReport 0x06 on 0xFF04 (Battery) ---");
        let mut buf = [0u8; 9];
        buf[0] = 0x06;
        match dev.get_feature_report(&mut buf) {
            Ok(n) => println!("Battery report: {} bytes: {:02X?}", n, &buf[..n]),
            Err(e) => println!("Battery report error: {}", e),
        }
    }
}
