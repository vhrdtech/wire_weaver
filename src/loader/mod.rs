enum Location {
    Local,
    Git,
    Github,
    // User?
}

pub struct Loader<'a> {
    pub fs_loader: Box<dyn FnMut() + 'a>

}

impl<'a> Loader<'a> {

    pub fn load(&mut self, uri: String) -> String {
        (self.fs_loader)();
        return "abc".to_string();
    }

    pub fn set_fs_loader(&mut self, loader: impl FnMut() + 'a) {
        self.fs_loader = Box::new(loader);
    }
}