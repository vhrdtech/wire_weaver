// enum Location {
//     Local,
//     Git,
//     Github,
//     // User?
// }

pub struct Loader<'a> {
    pub fs_loader: Box<dyn FnMut(u8) + 'a>,
}

impl<'a> Loader<'a> {
    pub fn load(&mut self, _uri: String) -> String {
        (self.fs_loader)(123);
        return "abc".to_string();
    }

    pub fn set_fs_loader(&mut self, loader: impl FnMut(u8) + 'a) {
        self.fs_loader = Box::new(loader);
    }
}
