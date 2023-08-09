use uuid::Uuid;

pub fn gun(qty: u8) -> String {
    let uuid_limit = match qty {
        0 => 1,
        x if x < 10 => x,
        _ => 9,
    };

    let mut uuids: Vec<(u8, Uuid)> = vec![];

    for idx in 0..uuid_limit {
        uuids.push((idx + 1, Uuid::new_v4()));
    }

    uuids
        .iter()
        .map(|(idx, u)| format!("{}\\.  `{}`", idx, u))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn gus() -> String {
    format!("`{}`", Uuid::new_v4())
}