/// PCM sample data (for the entire samples file)
pub struct SampleData {
    pub data : Vec<i8>,
}

impl SampleData {
    pub fn new(data : Vec<u8>) -> SampleData{
	let i8data = data.into_iter().map(|x| x as i8).collect();
	return SampleData { data : i8data };
    }
}
