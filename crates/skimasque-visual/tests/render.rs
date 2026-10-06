use skimasque_visual::{Component, Cx, Flow, Html, Node, NodeKind, Render};
use stucco_core::{el, to_html};

#[test]
fn components_and_owned_fragments_compose_in_stucco_elements() {
    let node = Node::new(NodeKind::Gateway).label("gateway <one>");
    let fragment = Node::new(NodeKind::Service).label("service & two").html();
    let html = to_html(&el::section().child(node).child(fragment));
    assert!(html.starts_with("<section><div class=\"v-node"));
    assert!(html.contains("gateway &lt;one&gt;"));
    assert!(html.contains("service &amp; two"));
    assert!(
        !html.contains("&lt;div"),
        "fragments must not be double escaped"
    );
    assert!(html.ends_with("</section>"));
}

struct DashboardLabel(String);

impl Render for DashboardLabel {
    fn render(&self, cx: &mut Cx) {
        el::strong().text(&self.0).render(cx);
    }
}

impl Component for DashboardLabel {}

#[test]
fn downstream_stucco_components_nest_in_visual_components() {
    let label = DashboardLabel("<script>alert(1)</script>".into());
    let fragment: Html = label.html();
    let html = Flow::new("Dashboard flow").then(&label).html();
    assert!(html.as_str().contains(fragment.as_str()));
    assert!(html.as_str().contains("<strong>&lt;script&gt;"));
    assert!(!html.as_str().contains("<script>"));
}
