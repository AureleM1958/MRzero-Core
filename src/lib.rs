use pyo3::gc::PyVisit;
use pyo3::prelude::*;
use pyo3::types::*;
use pyo3::{wrap_pyfunction, PyTraverseError};
mod pre_pass;
use pre_pass::{comp_graph, Repetition};
use std::{collections::HashMap, slice::from_raw_parts, time::Instant};

/// This is the building block of the graph generated by the pre-pass.
/// It contains everything needed to execute the graph.
#[pyclass(module = "_prepass")]
struct PyDistribution {
    #[pyo3(get, set)]
    dist_type: String,
    #[pyo3(get, set)]
    ancestors: Option<Py<PyList>>,
    // Only used in python, set to python's None
    #[pyo3(get, set)]
    mag: Option<PyObject>,
    #[pyo3(get, set)]
    kt_vec: Option<PyObject>,
    // Additional info for debugging, not needed for simulation
    #[pyo3(get)]
    prepass_mag: Option<Py<PyComplex>>,
    #[pyo3(get)]
    latent_signal: f32,
    #[pyo3(get)]
    signal: f32,
    #[pyo3(get)]
    rel_signal: f32,
    #[pyo3(get)]
    prepass_kt_vec: [f32; 4],
}

#[pymethods]
impl PyDistribution {
    #[new]
    fn new(py: Python) -> Self {
        PyDistribution {
            dist_type: "?".to_owned(),
            ancestors: Some(PyList::empty(py).into()),
            mag: Some(py.None()),
            kt_vec: Some(py.None()),
            prepass_mag: Some(PyComplex::from_doubles(py, 0.0, 0.0).into()),
            latent_signal: 0.0,
            signal: 0.0,
            rel_signal: 0.0,
            prepass_kt_vec: [0.0, 0.0, 0.0, 0.0],
        }
    }

    fn __traverse__(&self, visit: PyVisit) -> Result<(), PyTraverseError> {
        if let Some(ancestors) = &self.ancestors {
            visit.call(ancestors)?;
        }
        if let Some(mag) = &self.mag {
            visit.call(mag)?;
        }
        if let Some(kt_offset_vec) = &self.kt_vec {
            visit.call(kt_offset_vec)?;
        }
        if let Some(prepass_mag) = &self.prepass_mag {
            visit.call(prepass_mag)?;
        }
        Ok(())
    }

    fn __clear__(&mut self) {
        // Not all of these can create ref cycles, but better be safe than sorry
        self.ancestors = None;
        self.mag = None;
        self.kt_vec = None;
        self.prepass_mag = None;
    }

    fn __repr__(&self) -> String {
        Python::with_gil(|py| -> String {
            format!(
                "Dist(type: {}, mag: {:?}, signal: {}, kt: {:?}, #ancestors: {})",
                self.dist_type,
                self.prepass_mag,
                self.signal,
                self.prepass_kt_vec,
                match &self.ancestors {
                    Some(a) => a.as_ref(py).len(),
                    None => 0,
                },
            )
        })
    }
}

/// compute_graph(seq, t1, t2, t2dash, max_dist_count, min_dist_mag, nyquist, fov)
/// --
///
/// Computes a graph for the given sequence and parameters.
/// !!! Tensors are assumed to be 32 bit floats!
/// seq: Sequence that is simulated
/// t1: T1 relaxation time [s]
/// t2: T2 relaxation time [s]
/// t2dash: T2' dephasing time [s]
/// d: Diffusion coefficient [10^-3 mm^2/s]
/// max_dist_count: Maximum number of + or z distributions simulated
/// min_dist_mag: Minimum absolute magnetisation of a simulated distributions
/// nyquist: Frequency cutoff above no signal will be generated
/// fov: Size of FOV [m], needed for diffusion
#[pyfunction]
#[allow(clippy::too_many_arguments)]
fn compute_graph<'p>(
    py: Python<'p>,
    seq: &PyList,
    t1: f32,
    t2: f32,
    t2dash: f32,
    d: f32,
    max_dist_count: usize,
    min_dist_mag: f32,
    nyquist: [f32; 3],
    fov: [f32; 3],
    avg_b1_trig: &PyAny,
) -> PyResult<&'p PyList> {
    println!(">>>> Rust - compute_graph(...) >>>");
    let start = Instant::now();
    let mut sequence = Vec::new();

    let tensor = avg_b1_trig.getattr("cpu")?.call0()?;
    let size: usize = tensor.getattr("numel")?.call0()?.extract()?;
    // NOTE: We should also check if the type is torch.float32
    assert_eq!(size, 3 * 361);
    let data_ptr: usize = tensor.getattr("data_ptr")?.call0()?.extract()?;
    let avg_b1_trig = unsafe { from_raw_parts(data_ptr as *mut [f32; 3], 361).to_vec() };

    for rep in seq.iter() {
        let pulse = rep.getattr("pulse")?;
        let pulse_angle: f32 = pulse.getattr("angle")?.extract()?;
        let pulse_phase: f32 = pulse.getattr("phase")?.extract()?;

        let event_count: usize = rep.getattr("event_count")?.extract()?;
        let data_ptr = |name: &str| -> PyResult<usize> {
            let tensor = rep.getattr(name)?.getattr("cpu")?.call0()?;
            let data_ptr: usize = tensor.getattr("data_ptr")?.call0()?.extract()?;
            Ok(data_ptr)
        };

        let event_time =
            unsafe { from_raw_parts(data_ptr("event_time")? as *mut f32, event_count).to_vec() };
        let gradm_event =
            unsafe { from_raw_parts(data_ptr("gradm")? as *mut [f32; 3], event_count).to_vec() };
        let adc_usage =
            unsafe { from_raw_parts(data_ptr("adc_usage")? as *mut i32, event_count).to_vec() };

        let repetition = Repetition {
            pulse_angle,
            pulse_phase,
            event_time,
            gradm_event,
            adc_mask: adc_usage.iter().map(|&usage| usage > 0).collect(),
        };

        sequence.push(repetition);
    }

    println!(
        "Converting Python -> Rust: {} s",
        start.elapsed().as_secs_f32()
    );
    let start = Instant::now();
    println!("Compute Graph");

    let tau = std::f32::consts::TAU;
    let mut graph = comp_graph(
        &sequence,
        t1,
        t2,
        t2dash,
        d * 1e-9, // convert to m²/s
        max_dist_count,
        min_dist_mag,
        nyquist,
        [tau / fov[0], tau / fov[1], tau / fov[2]], // convert to 1/m,
        &avg_b1_trig,
    );

    println!("Computing Graph: {} s", start.elapsed().as_secs_f32());
    let start = Instant::now();

    println!("Analyze Graph");
    pre_pass::analyze_graph(&mut graph);
    println!("Analyzing Graph: {} s", start.elapsed().as_secs_f32());
    let start = Instant::now();

    let mut mapping: HashMap<usize, Py<PyDistribution>> = HashMap::new();
    let address = |rc: &pre_pass::RcDist| rc.as_ptr() as usize;

    // Create a PyDistribution for every Distribution and store the mapping
    for rep in graph.iter() {
        for (dist, addr) in rep.iter().map(|d| (d.borrow(), address(d))) {
            // Create the PyDistribution
            let mut py_dist = PyDistribution::new(py);

            // Pass the dist type to python as string
            py_dist.dist_type = pre_pass::DIST_TYPE_STR[dist.dist_type as usize].to_owned();

            // Use the mapping to search for ancestors and create python references
            for ancestor in dist.ancestors.iter() {
                let py_ancestor = mapping
                    .get(&address(&ancestor.dist))
                    .expect("Setting ancestors: Dist doesn't exist in the Rust -> Python mapping")
                    .clone_ref(py);
                py_dist
                    .ancestors
                    .as_ref()
                    .expect("Ancestor was not set")
                    .as_ref(py)
                    .append(PyTuple::new(
                        py,
                        &[
                            PyString::new(
                                py,
                                pre_pass::DIST_RELATION_STR[ancestor.relation as usize],
                            )
                            .as_ref(),
                            py_ancestor.as_ref(py),
                            PyComplex::from_doubles(
                                py,
                                ancestor.rot_mat_factor.re as f64,
                                ancestor.rot_mat_factor.im as f64,
                            )
                            .as_ref(),
                        ],
                    ))?;
            }

            // Extra debugging info
            py_dist.prepass_mag = {
                let mag = dist.mag;
                Some(PyComplex::from_doubles(py, mag.re as f64, mag.im as f64).into())
            };
            py_dist.latent_signal = dist.latent_signal;
            py_dist.signal = dist.signal;
            py_dist.rel_signal = dist.rel_signal;
            py_dist.prepass_kt_vec = dist.kt_vec;
            // Insert the PyDistribution into map
            mapping.insert(
                addr,
                Py::new(py, py_dist)
                    .expect("Failed to create a Python object out of a PyDistribution"),
            );
        }
    }

    let py_graph = PyList::new(
        py,
        graph.into_iter().map(|rep| {
            PyList::new(
                py,
                rep.into_iter().map(|dist| {
                    mapping.get(&address(&dist)).expect(
                        "Converting graph: Dist doesn't exist in the Rust -> Python mapping",
                    )
                }),
            )
        }),
    );

    println!(
        "Converting Rust -> Python: {} s",
        start.elapsed().as_secs_f32()
    );
    println!("<<<< Rust <<<<");

    Ok(py_graph)
}

/// A Python module implemented in Rust.
#[pymodule]
fn _prepass(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(compute_graph, m)?)?;
    m.add_class::<PyDistribution>()?;

    Ok(())
}
