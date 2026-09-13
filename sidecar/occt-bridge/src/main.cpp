// occt-bridge — see ../README.md. Only `version` and `selftest` exist yet: this is the Phase 0b
// spike that proves OCCT builds, links and runs in the worker image. `convert` and
// `generate-fixtures` follow once it does.

#include <BRepGProp.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <GProp_GProps.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <NCollection_Sequence.hxx>
#include <STEPCAFControl_Reader.hxx>
#include <STEPCAFControl_Writer.hxx>
#include <Standard_Version.hxx>
#include <TDF_Label.hxx>
#include <TDocStd_Document.hxx>
#include <TopoDS_Shape.hxx>
#include <XCAFApp_Application.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <XCAFDoc_ShapeTool.hxx>

#include <cmath>
#include <cstdio>
#include <cstring>
#include <string>

namespace {

// Bumped whenever the bridge changes what it writes. Together with the OCCT version it is the
// kernel version the worker fleet pins: two builds that tessellate differently must not
// produce derivatives that are cached as the same.
constexpr int BRIDGE_VERSION = 0;

const double PI = std::acos(-1.0);

int version() {
  std::printf("occt %s bridge %d\n", OCC_VERSION_COMPLETE, BRIDGE_VERSION);
  return 0;
}

// Writes a 22 mm cylinder as STEP through XCAF, reads it back, and checks that the volume
// survived the round trip and that the shape meshes. Every toolkit a real conversion needs is
// on this path, so a missing library fails here, at image build time.
int selftest(const char* dir) {
  const double radius = 11.0;
  const double height = 30.0;
  occ::handle<XCAFApp_Application> app = XCAFApp_Application::GetApplication();

  occ::handle<TDocStd_Document> written;
  app->NewDocument("MDTV-XCAF", written);
  const TopoDS_Shape cylinder = BRepPrimAPI_MakeCylinder(radius, height).Shape();
  XCAFDoc_DocumentTool::ShapeTool(written->Main())->AddShape(cylinder);

  const std::string path = std::string(dir) + "/occt-bridge-selftest-cylinder-d22.step";
  STEPCAFControl_Writer writer;
  if (!writer.Transfer(written) || writer.Write(path.c_str()) != IFSelect_RetDone) {
    std::fprintf(stderr, "selftest: could not write %s\n", path.c_str());
    return 1;
  }

  STEPCAFControl_Reader reader;
  if (reader.ReadFile(path.c_str()) != IFSelect_RetDone) {
    std::fprintf(stderr, "selftest: could not read back %s\n", path.c_str());
    return 1;
  }
  occ::handle<TDocStd_Document> read;
  app->NewDocument("MDTV-XCAF", read);
  if (!reader.Transfer(read)) {
    std::fprintf(stderr, "selftest: the STEP read but did not transfer into a document\n");
    return 1;
  }

  NCollection_Sequence<TDF_Label> roots;
  XCAFDoc_DocumentTool::ShapeTool(read->Main())->GetFreeShapes(roots);
  if (roots.Length() != 1) {
    std::fprintf(stderr, "selftest: expected 1 root shape, read %d\n", roots.Length());
    return 1;
  }
  const TopoDS_Shape shape = XCAFDoc_ShapeTool::GetShape(roots.Value(1));

  GProp_GProps props;
  BRepGProp::VolumeProperties(shape, props);
  const double expected = PI * radius * radius * height;
  const BRepMesh_IncrementalMesh mesh(shape, 0.05);

  const bool volumeHolds = std::fabs(props.Mass() - expected) <= 1e-6 * expected;
  std::printf("selftest: roots=%d volume=%.6f expected=%.6f meshed=%s\n", roots.Length(),
              props.Mass(), expected, mesh.IsDone() ? "yes" : "no");
  return volumeHolds && mesh.IsDone() ? 0 : 1;
}

int usage() {
  std::fprintf(stderr, "usage: occt-bridge version | occt-bridge selftest <scratch-dir>\n");
  return 64;
}

}  // namespace

int main(int argc, char** argv) {
  if (argc >= 2 && std::strcmp(argv[1], "version") == 0) {
    return version();
  }
  if (argc >= 3 && std::strcmp(argv[1], "selftest") == 0) {
    return selftest(argv[2]);
  }
  return usage();
}
