//! Pure, read-only Modbus request handling over a fixed register bank.
//! Supports FC03 (Read Holding Registers) and FC04 (Read Input Registers).

const FC_READ_HOLDING: u8 = 0x03;
const FC_READ_INPUT: u8 = 0x04;

/// Handle one full ADU (MBAP header + PDU). Returns the full response ADU.
/// `holding_mirror` = whether FC03 is served (else FC03 -> illegal function).
pub fn handle_adu(bank: &[u16], holding_mirror: bool, unit_id: u8, req: &[u8]) -> Vec<u8> {
    // MBAP: [tx_hi, tx_lo, proto_hi, proto_lo, len_hi, len_lo, unit] then PDU.
    if req.len() < 8 {
        return Vec::new();
    }
    let tx = [req[0], req[1]];
    let req_unit = req[6];
    let pdu = &req[7..];
    let fc = pdu[0];

    let make = |payload: Vec<u8>| -> Vec<u8> {
        let len = (payload.len() + 1) as u16; // +1 for unit id
        let mut out = Vec::with_capacity(7 + payload.len());
        out.extend_from_slice(&tx);
        out.extend_from_slice(&[0, 0]); // protocol id
        out.extend_from_slice(&len.to_be_bytes());
        out.push(req_unit);
        out.extend_from_slice(&payload);
        out
    };
    let exception = |code: u8| make(vec![fc | 0x80, code]);

    // Accept our configured unit id or the broadcast/wildcard 0.
    if req_unit != unit_id && req_unit != 0 {
        return Vec::new();
    }

    match fc {
        FC_READ_HOLDING if !holding_mirror => exception(0x01),
        FC_READ_HOLDING | FC_READ_INPUT => {
            if pdu.len() < 5 {
                return exception(0x03);
            }
            let start = u16::from_be_bytes([pdu[1], pdu[2]]) as usize;
            let qty = u16::from_be_bytes([pdu[3], pdu[4]]) as usize;
            if qty == 0 || qty > 125 {
                return exception(0x03);
            }
            if start + qty > bank.len() {
                return exception(0x02);
            }
            let mut payload = vec![fc, (qty * 2) as u8];
            for w in &bank[start..start + qty] {
                payload.extend_from_slice(&w.to_be_bytes());
            }
            make(payload)
        }
        _ => exception(0x01),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_req(fc: u8, unit: u8, start: u16, qty: u16) -> Vec<u8> {
        let mut v = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x06, unit, fc];
        v.extend_from_slice(&start.to_be_bytes());
        v.extend_from_slice(&qty.to_be_bytes());
        v
    }

    #[test]
    fn reads_input_registers() {
        let bank = vec![0x1234u16, 0x5678, 0x9ABC];
        let resp = handle_adu(&bank, true, 1, &read_req(FC_READ_INPUT, 1, 0, 2));
        assert_eq!(resp[7], FC_READ_INPUT);
        assert_eq!(resp[8], 4);
        assert_eq!(&resp[9..13], &[0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn holding_disabled_returns_exception() {
        let bank = vec![0u16; 4];
        let resp = handle_adu(&bank, false, 1, &read_req(FC_READ_HOLDING, 1, 0, 1));
        assert_eq!(resp[7], FC_READ_HOLDING | 0x80);
        assert_eq!(resp[8], 0x01);
    }

    #[test]
    fn out_of_range_returns_exception_02() {
        let bank = vec![0u16; 4];
        let resp = handle_adu(&bank, true, 1, &read_req(FC_READ_INPUT, 1, 3, 5));
        assert_eq!(resp[7], FC_READ_INPUT | 0x80);
        assert_eq!(resp[8], 0x02);
    }

    #[test]
    fn unknown_function_returns_exception_01() {
        let bank = vec![0u16; 4];
        let req = read_req(0x10, 1, 0, 1);
        let resp = handle_adu(&bank, true, 1, &req);
        assert_eq!(resp[7], 0x10 | 0x80);
        assert_eq!(resp[8], 0x01);
    }

    #[test]
    fn wrong_unit_id_is_ignored() {
        let bank = vec![0u16; 4];
        let resp = handle_adu(&bank, true, 1, &read_req(FC_READ_INPUT, 9, 0, 1));
        assert!(resp.is_empty());
    }
}
