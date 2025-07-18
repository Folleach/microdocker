// this file contains models from OCI and destribution

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
pub struct Index {
	#[serde(alias = "MediaType")]
	pub media_type: Option<String>,

	#[serde(alias = "ArtifactType")]
	pub artifact_type: Option<String>,

	pub manifests: Vec<Descriptor>,

	pub subject: Option<Descriptor>,

	pub annotations: Option<HashMap<String, String>>
}

#[derive(Deserialize, Debug, Default)]
pub struct Manifest {
    #[serde(alias = "MediaType")]
    pub media_type: Option<String>,
    pub config: Option<Descriptor>,
    pub layers: Vec<Descriptor>
}

#[derive(Deserialize, Debug)]
pub struct Descriptor {
    #[serde(alias = "mediaType")]
    pub media_type: Option<String>,
    pub digest: Option<String>,
    pub size: Option<i64>,
    #[serde(alias = "URLs")]
    pub urls: Option<Vec<String>>,
    pub annotations: Option<HashMap<String, String>>,
}

// https://github.com/opencontainers/image-spec/blob/main/specs-go/v1/config.go
#[derive(Serialize, Deserialize, Debug)]
pub struct  ImageConfig {
	#[serde(alias = "User")]
	pub user: Option<String>,

	#[serde(alias = "ExposedPorts")]
	pub exposed_ports: Option<HashMap<String, HashMap<String, String>>>,

	#[serde(alias = "Env")]
	pub env: Option<Vec<String>>,

	#[serde(alias = "Entrypoint")]
	pub entrypoint: Option<Vec<String>>,

	#[serde(alias = "Cmd")]
	pub cmd: Option<Vec<String>>,

	#[serde(alias = "Volumes")]
	pub volumes: Option<HashMap<String, ()>>,

	#[serde(alias = "WorkingDir")]
	pub working_dir: Option<String>,

	#[serde(alias = "Labels")]
	pub labels: Option<HashMap<String, String>>,

	#[serde(alias = "StopSignal")]
	pub stop_signal: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct RootFS {
	#[serde(alias = "type")]
	pub fstype: String,

	pub diff_ids: Vec<String>
}

#[derive(Serialize, Deserialize, Debug)]
pub struct History {
	// created: Option<?>

	pub created_by: Option<String>,

	pub author: Option<String>,

	pub comment: Option<String>,

	pub empty_layer: Option<bool>
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Image {
	// created: Option<?>

	pub author: Option<String>,

	// platform

	pub config: Option<ImageConfig>,

	#[serde(alias = "rootfs")]
	pub root_fs: RootFS,

	pub history: Option<Vec<History>>
}
