impl ItemStruct {
    pub fn contains_ref_types(&self) -> bool {
        for f in &self.fields {
            if f.ty.is_ref() {
                return true;
            }
        }
        false
    }
}

impl ItemEnum {
    pub fn contains_data_fields(&self) -> bool {
        for variant in &self.variants {
            match variant.fields {
                Fields::Named(_) => {
                    return true;
                }
                Fields::Unnamed(_) => {
                    return true;
                }
                Fields::Unit => {}
            }
        }
        false
    }
}
