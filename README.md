MRzero Core contains all essential parts of MRzero that are (close to) finalized.
Over time, more and more functionality should be pushed from MRzero to the Core,
while experimental scripts and WIP functionality stays in the MRzero git.
Everything contained in the Core should be documented, have a stable API and,
in the near future, be tested.

# BUILDING

Building for linux still can use some improvements. Current solution:
```
docker run --rm -v D:/repos/MRzero-Core:/io konstin2/maturin build --release
```
This builds a manylinux verison of the wheel but fails to add all the python scripts to it.
Copy the .dist-info folder and the _prepass.abi3.so file into a windows wheel.

# CONTENTS

The MRzero Core contains the following, which can be imported with e.g.:
```python
from MRzeroCore.phantom import VoxelGridPhantom
```

> NOTE:
>
> This list is currently WIP and does not reflect the actual state of MRzero Core.
> Before publishing v1.0 of MRzero Core, it should be changed to reflect what is
> currently contained and what is still TODO.

### MRzeroCore
- phantom
    - CustomVoxelPhantom
    - VoxelGridPhantom
    - SimData
- pulseq
    - Pulseq Interpreter
    - Pulseq Sequence Exporter
- reconstruction
    - Adjoint
    - FFT
    - NUFFT (requires torchkbnufft dependency)
    - Grappa
- sequence
    - Tools for sequence design and visualisation
    - Templates for simple GRE, TSE, ... sequences
- simulation
    - Prepass
    - PDG
    - Isochromats
