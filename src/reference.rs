#[derive(Debug)]
pub struct SimpleReference {
    pub path: String,
    pub tag: Option<String>,
    pub hash: Option<String>
}

// https://github.com/distribution/reference/blob/main/normalize.go#L10-L40
const DEFAULT_DOMAIN: &str = "docker.io";
const DEFAULT_PREFIX: &str = "library/";
const DEFAULT_TAG: &str = "latest";

impl SimpleReference {
    pub fn tag(&self) -> String {
        if let Some(v) = &self.hash { return v.clone(); }
        if let Some(v) = &self.tag { return v.clone(); }
        DEFAULT_TAG.to_string()
    }

    pub fn with_hash(&self, hash: &str) -> SimpleReference {
        SimpleReference {
            path: self.path.clone(),
            tag: self.tag.clone(),
            hash: Some(hash.to_string())
        }
    }

    pub fn parse(value: &str) -> SimpleReference {
        let array: Vec<&str> = value.split(":").collect();
        SimpleReference {
            path: array[0].to_string(),
            tag: if array.len() > 1 { Some(array[1].to_string()) } else { None },
            hash: None
        }
    }

    pub fn to_string(&self) -> String {
        let mut string = String::new();

        string.push_str(&self.path);
        string.push_str(":");
        string.push_str(&self.tag());

        string.to_string()
    }
}
