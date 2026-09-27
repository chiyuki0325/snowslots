use slot_store::{sort_key, Cart, Platform};
use slot_ui::{Shelf, ShelfSort, TexId};

fn cart(stem: &str, last_launched: Option<i64>) -> Cart {
    Cart {
        stem: stem.into(),
        platform: Platform::Gba,
        rom: format!("Games/GBA/{stem}.gba").into(),
        label: None,
        title: stem.to_uppercase(),
        code: String::new(),
        last_launched,
    }
}

fn stems(shelf: &Shelf) -> Vec<&str> {
    shelf.carts.iter().map(|cart| cart.stem.as_str()).collect()
}

#[test]
fn chinese_names_file_with_their_plain_pinyin() {
    let mut names = vec!["Zelda", "宝可梦", "Advance Wars", "马力欧"];
    names.sort_by_key(|name| sort_key(name));
    assert_eq!(names, ["Advance Wars", "宝可梦", "马力欧", "Zelda"]);
}

#[test]
fn sort_modes_cycle_both_ways_and_keep_the_selected_cart() {
    let mut shelf = Shelf::new(vec![
        cart("Alpha", Some(10)),
        cart("Bravo", Some(30)),
        cart("Charlie", None),
    ]);
    shelf.select(0);

    assert_eq!(shelf.cycle_sort(1), ShelfSort::Recent);
    assert_eq!(stems(&shelf), ["Bravo", "Alpha", "Charlie"]);
    assert_eq!(shelf.carts[shelf.index].stem, "Alpha");

    assert_eq!(shelf.cycle_sort(1), ShelfSort::System);
    assert_eq!(stems(&shelf), ["Alpha", "Bravo", "Charlie"]);
    assert_eq!(shelf.cycle_sort(-1), ShelfSort::Recent);
}

#[test]
fn faces_remain_attached_to_their_carts_after_a_reorder() {
    let mut shelf = Shelf::new(vec![cart("Alpha", Some(10)), cart("Bravo", Some(30))]);
    shelf.set_faces(vec![TexId::from_raw(1), TexId::from_raw(2)]);
    shelf.cycle_sort(1);

    assert_eq!(shelf.find("Alpha").unwrap().1, Some(TexId::from_raw(1)));
    assert_eq!(shelf.find("Bravo").unwrap().1, Some(TexId::from_raw(2)));
}

#[test]
fn recent_navigation_crosses_local_days_and_the_unplayed_group() {
    let day = 86_400;
    let mut shelf = Shelf::new(vec![
        cart("Old morning", Some(day + 60)),
        cart("Today evening", Some(2 * day + 20_000)),
        cart("Today morning", Some(2 * day + 100)),
        cart("Never", None),
    ]);
    shelf.cycle_sort(1);
    assert_eq!(
        stems(&shelf),
        ["Today evening", "Today morning", "Old morning", "Never"]
    );
    shelf.select(0);

    shelf.jump_next_group(0);
    assert_eq!(shelf.carts[shelf.index].stem, "Old morning");
    shelf.jump_next_group(0);
    assert_eq!(shelf.carts[shelf.index].stem, "Never");
    shelf.jump_next_group(0);
    assert_eq!(shelf.carts[shelf.index].stem, "Today evening");
}

#[test]
fn recording_a_launch_reorders_recent_without_losing_selection() {
    let mut shelf = Shelf::new(vec![cart("Alpha", Some(10)), cart("Bravo", Some(20))]);
    shelf.cycle_sort(1);
    let alpha = shelf
        .carts
        .iter()
        .position(|cart| cart.stem == "Alpha")
        .unwrap();
    shelf.select(alpha);
    shelf.record_launched(Platform::Gba, "Alpha", 30);

    assert_eq!(stems(&shelf), ["Alpha", "Bravo"]);
    assert_eq!(shelf.carts[shelf.index].stem, "Alpha");
}

#[test]
fn one_system_has_no_other_group_to_visit() {
    let mut shelf = Shelf::new(vec![cart("Alpha", None), cart("Bravo", None)]);
    shelf.cycle_sort(-1);
    assert_eq!(shelf.sort_mode(), ShelfSort::System);
    shelf.jump_next_group(0);
    assert_eq!(shelf.index, 0);
}
