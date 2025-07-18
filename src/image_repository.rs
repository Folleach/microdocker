use std::{error::Error, fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{common::create_if_not_exists, models::{Image, Manifest}, reference::SimpleReference};

const REPOSITORY_FILENAME: &str = "images.json";

#[derive(Serialize, Deserialize)]
pub struct ImageDescriptor {
    names: Vec<String>,
    config: String
}

#[derive(Serialize, Deserialize)]
pub struct ImagesList {
    images: Vec<ImageDescriptor>
}

pub struct ImageRepository {
    path: PathBuf
}

impl ImageRepository {
    pub fn new(path: PathBuf) -> ImageRepository {
        ImageRepository { path: path }
    }

    pub fn get(&self, reference: &SimpleReference) -> Result<Option<Image>, Box<dyn Error>> {
        let repository = self.load()?;

        let descriptor = repository.images.iter().find(|x| x.names.contains(&reference.to_string()));
        let descriptor = match descriptor {
            Some(v) => v,
            None => return Ok(None)
        };

        let image_data = fs::read_to_string(self.path.join(&descriptor.config).join("image"))?;
        let image = serde_json::from_str(&image_data)?;

        Ok(image)
    }

    pub fn append(&self, references: Vec<&SimpleReference>, manifest: &Manifest, image: &Image) -> Result<(), Box<dyn Error>> {
        let mut repository = self.load()?;

        let id = match &manifest.config {
            Some(v) => match &v.digest {
                Some(v) => v,
                None => return Err("config does not provide digest".into())
            },
            None => return Err("manifest does not provide config".into())
        };
        let descriptor = ImageDescriptor {
            names: references.iter().map(|x| x.to_string()).collect(),
            config: id.to_owned()
        };
        repository.images.push(descriptor);

        let path = create_if_not_exists(self.path.join(&id))?;
        let image_data = serde_json::to_string_pretty(&image)?;
        fs::write(path.join("image"), &image_data)?;

        return self.store(&repository);
    }

    fn load(&self) -> Result<ImagesList, Box<dyn Error>> {
        let data = fs::read_to_string(self.path.join(REPOSITORY_FILENAME));
        let repository = match data {
            Ok(v) => serde_json::from_str(&v)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let repository = ImagesList {
                    images: vec![]
                };
                self.store(&repository)?;
                repository
            }
            Err(e) => {
                return Err(format!("failed to read images repository: {}", e).into())
            }
        };
        Ok(repository)
    }

    fn store(&self, repository: &ImagesList) -> Result<(), Box<dyn Error>> {
        let data = serde_json::to_string_pretty(&repository)?;
        fs::write(self.path.join(REPOSITORY_FILENAME), data)?;
        Ok(())
    }
}
