# occt-bridge: the one program in Lapidary that links Open CASCADE. A separate process rather
# than a library in the worker, so an OCCT crash takes down one job and not the worker.
cmake_minimum_required(VERSION 3.16)
project(occt-bridge LANGUAGES CXX)

# OCCT 8.0 requires C++17.
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

find_package(OpenCASCADE REQUIRED CONFIG)

add_executable(occt-bridge src/main.cpp)
target_include_directories(occt-bridge PRIVATE ${OpenCASCADE_INCLUDE_DIR})
# Named toolkit by toolkit rather than by module, so the link line is the list of what the
# bridge actually calls: STEP and IGES through XCAF, primitives and booleans for the fixture
# generator, meshing, and mass properties and bounding boxes.
target_link_libraries(occt-bridge PRIVATE
  TKDESTEP TKDEIGES TKDE TKXSBase
  TKXCAF TKCAF TKLCAF TKCDF
  TKBO TKMesh TKPrim TKTopAlgo TKGeomAlgo TKGeomBase
  TKBRep TKG3d TKG2d TKMath TKernel
)
