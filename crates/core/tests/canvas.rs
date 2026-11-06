use fdl_core::{canvas::{Axis, Drawing, Endpoint, Object, Port, Route, Shape}, docxml, editor::core::Core, markdown, model::*};

#[test]
fn canvas_history_and_table_round_trip_keep_editable_nodes_and_arrows() {
    let mut core = Core::new(Document::default());
    let id = core.insert_canvas(Drawing::default());
    let mut drawing = Drawing::default();
    let start = drawing.add_node(Shape::Rectangle, [145, 150]).unwrap();
    let end = drawing.add_node(Shape::Diamond, [370, 150]).unwrap();
    drawing.label_node(start, "开始 <&>");
    drawing.label_node(end, "检查");
    let edge = drawing.connect(start, end).unwrap();
    assert!(drawing.connect(start, end).is_none());
    assert!(core.set_canvas(id, drawing.clone(), None));
    let original_tip = drawing.arrow(drawing.edge(edge).unwrap()).unwrap().tip();
    let mut moved = drawing.clone();
    moved.move_node(end, [420, 260]);
    assert_ne!(moved.arrow(moved.edge(edge).unwrap()).unwrap().tip(), original_tip);
    assert!(core.set_canvas(id, moved.clone(), None));
    let mut removed = moved.clone();
    removed.remove(Object::Node(end));
    assert!(removed.edges().is_empty());
    assert!(core.set_canvas(id, removed, None));
    assert!(core.undo());
    assert!(matches!(core.kind_of(id), Some(BlockKind::Canvas(d)) if *d == moved));
    assert!(core.redo());
    assert!(core.undo());
    let saved = docxml::serialize(&core.doc);
    let inline = saved.lines().find(|line| line.starts_with("<quiet-canvas ")).unwrap();
    // A previous reader may wrap the unrecognized block in a paragraph.
    let wrapped = format!("<table><tbody><tr><td><p>{inline}</p></td></tr></tbody></table>");
    let reopened = docxml::parse(&wrapped).unwrap();
    let mut preserved = false;
    reopened.for_each_block(|b| if let BlockKind::Canvas(d) = &b.kind { preserved |= *d == moved; });
    assert!(preserved);
    assert!(reopened.plain_text().contains("开始 <&>"));
    let exported = markdown::export(&core.doc, &markdown::Options { assets: Some("test.assets"), ..Default::default() });
    assert_eq!(exported.canvases.len(), 1);
    assert_eq!(exported.canvases[0].0, moved);
    assert!(exported.text.contains(&exported.canvases[0].1));
}

#[test]
fn unsupported_or_invalid_canvas_records_preserve_the_raw_block() {
    for tag in [
        r#"<quiet-canvas version="3" data="future"/>"#,
        r#"<quiet-canvas version="1" data="{&quot;nodes&quot;:[],&quot;edges&quot;:[{&quot;from&quot;:1,&quot;to&quot;:2}]}"/>"#,
    ] {
        let document = docxml::parse(tag).unwrap();
        assert!(matches!(document.blocks[0].kind, BlockKind::Raw { .. }));
        assert!(docxml::serialize(&document).contains(tag));
    }
}

#[test]
fn legacy_canvas_is_kept_until_its_first_actual_edit() {
    let record = r#"{ "nodes":[{"id":1,"shape":"rectangle","center":[120,120],"label":"旧图"}],"edges":[] }"#;
    let mut drawing = Drawing::from_record("1", record).unwrap();
    assert_eq!(drawing.node(1).unwrap().size, Shape::Rectangle.size());
    drawing.move_node(1, [120, 120]);
    drawing.label_node(1, "旧图");
    assert_eq!(drawing.version(), "1");
    assert_eq!(drawing.record(), record);
    drawing.move_node(1, [-340, 780]);
    drawing.label_node(1, "第一行\n第二行");
    assert_eq!(drawing.version(), "2");
    assert_eq!(Drawing::from_record("2", &drawing.record()), Some(drawing));
}

#[test]
fn layout_and_rebinding_keep_port_identity_and_branch_text() {
    let mut drawing = Drawing::default();
    let a = drawing.add_node(Shape::Rectangle, [-200, 100]).unwrap();
    let b = drawing.add_node(Shape::Diamond, [120, 250]).unwrap();
    let c = drawing.add_node(Shape::Circle, [400, -100]).unwrap();
    let from = Endpoint { node: a, port: Port::Right };
    let to = Endpoint { node: b, port: Port::Top };
    let wire = drawing.connect_ports(from, to).unwrap();
    drawing.label_edge(wire, "是");
    drawing.toggle_route(wire);
    drawing.resize_node(c, [400, -100], [150, 110]);
    assert_eq!(drawing.node(c).unwrap().size, [150, 150]);
    assert!(drawing.reconnect(wire, false, Endpoint { node: c, port: Port::Bottom }));
    let seed = drawing.edge(wire).unwrap().seed;
    drawing.arrange(&[a, b, c], Axis::Y, true);
    drawing.arrange(&[a, b], Axis::X, false);
    let [a_bounds, b_bounds] = [a, b].map(|id| drawing.node(id).unwrap().bounds());
    assert_eq!(a_bounds.min[0], b_bounds.min[0]);
    drawing.translate(&[a, b, c], [1200, -800]);
    let edge = drawing.edge(wire).unwrap();
    assert_eq!((edge.seed, edge.route, edge.label.as_str()), (seed, Route::Elbow, "是"));
    let arrow = drawing.arrow(edge).unwrap();
    assert_eq!(arrow.tail(), drawing.node(a).unwrap().port(Port::Right));
    assert_eq!(arrow.tip(), drawing.node(c).unwrap().port(Port::Bottom));
    assert!(arrow.points.windows(2).all(|p| p[0][0] == p[1][0] || p[0][1] == p[1][1]));
    assert_eq!(Drawing::from_record(drawing.version(), &drawing.record()), Some(drawing));
}
