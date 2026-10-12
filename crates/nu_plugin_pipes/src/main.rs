use nu_plugin::{MsgPackSerializer, serve_plugin};
use nu_plugin_pipes::PipesPlugin;

fn main() {
    serve_plugin(&PipesPlugin::default(), MsgPackSerializer);
}
