use slot_ui::{menu_face, QuickRow, Toast};

#[test]
fn the_label_font_contains_the_chinese_ui_glyphs() {
    let font = slot_ui::text::label_font().expect("label font");
    let labels = QuickRow::ALL
        .into_iter()
        .map(|row| row.label().to_string())
        .chain(Toast::all().into_iter().map(Toast::text))
        .chain(
            [
                "最近游戏 收藏 全部游戏 新建合集",
                "正在搜索网络 打开Wi-Fi后可查看网络 输入密码 自动连接 忘记网络",
                "正在验证 正在获取IP地址 密码错误",
                "正在检查更新 存储卡空间不足 下载文件已损坏",
                "扫描二维码 或输入配对码 等待配对 服务器 端口 昵称",
                "键盘帮助 攻略帮助 大小写 自动换行 平移 勾选",
                "标签打印 外壳颜色 材质 不透明 闪粉 缺口 圆角",
                "服务器未开启安装功能 配对授权缺少所需权限",
            ]
            .into_iter()
            .map(str::to_string),
        );
    for label in labels {
        for character in label.chars().filter(|character| !character.is_whitespace()) {
            assert_ne!(
                font.lookup_glyph_index(character),
                0,
                "{label}: {character}"
            );
        }
    }
}

#[test]
fn chinese_menu_faces_are_not_blank_or_clipped() {
    for label in [
        "日期与时间",
        "保存状态",
        "宝可梦",
        "儿童游戏架",
        "新建合集",
        "自动连接",
        "键盘帮助",
        "标签打印",
    ] {
        let face = menu_face(label);
        assert!(
            face.rgba.chunks_exact(4).any(|pixel| pixel[3] > 0),
            "{label}"
        );
        for row in [0, face.h - 1] {
            assert!(
                (0..face.w)
                    .all(|column| face.rgba[((row * face.w + column) * 4 + 3) as usize] == 0),
                "{label} touches the vertical edge"
            );
        }
    }
}
