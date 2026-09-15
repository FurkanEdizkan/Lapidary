// occt-bridge — see ../README.md.
//
//   occt-bridge version
//   occt-bridge selftest <scratch-dir>
//   occt-bridge convert --in <file> --format step|iges --out <dir> [--deflection <mm>]
//   occt-bridge generate-fixtures <dir>
//
// `convert` writes four files into <dir> and prints a one-line JSON summary on stdout:
//
//   mesh.stl           every placed part triangulated in world coordinates, as binary STL, so
//                      the worker's existing mesh pipeline — clustering, thumbnail, GLB — reads
//                      it instead of this program growing a second one
//   parts.json         how many of mesh.stl's triangles each placed part has, in the order
//                      structure.json lists its leaves, so a viewer can hide one part
//   pmi.json           the dimensions, geometric tolerances and datums an AP242 file specifies,
//                      each naming the prototype and face it applies to
//   structure.json     the assembly tree: names, prototypes, 4x4 transforms relative to parent
//   entities.json      analytic faces and circular edges, once per prototype, in its own
//                      coordinates; structure.json places them
//   measurements.json  volume, surface area, bounding box, faces and edges from the B-rep, in millimetres
//   header.json        what the file says about itself: its STEP header or IGES global
//                      section, and the materials it names
//
// A file OCCT cannot read exits 2 with {"kind":"refused","detail":...} on stderr. Anything
// else non-zero, or a signal, is a crash, and the worker treats it as one.

#include <APIHeaderSection_MakeHeader.hxx>
#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepBndLib.hxx>
#include <BRepBuilderAPI_MakeSolid.hxx>
#include <BRepBuilderAPI_Sewing.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepGProp.hxx>
#include <BRepLib.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRep_Builder.hxx>
#include <BRep_Tool.hxx>
#include <Bnd_Box.hxx>
#include <DESTEP_Parameters.hxx>
#include <GProp_GProps.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <IGESCAFControl_Reader.hxx>
#include <IGESCAFControl_Writer.hxx>
#include <IGESData_GlobalSection.hxx>
#include <IGESData_IGESModel.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_PrinterOStream.hxx>
#include <NCollection_IndexedMap.hxx>
#include <NCollection_Sequence.hxx>
#include <Poly_Triangulation.hxx>
#include <STEPCAFControl_Reader.hxx>
#include <STEPCAFControl_Writer.hxx>
#include <Standard_Failure.hxx>
#include <Standard_Version.hxx>
#include <StepData_StepModel.hxx>
#include <TCollection_AsciiString.hxx>
#include <TCollection_ExtendedString.hxx>
#include <TCollection_HAsciiString.hxx>
#include <TDF_Label.hxx>
#include <TDF_Tool.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Document.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopLoc_Location.hxx>
#include <TopTools_ShapeMapHasher.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Shape.hxx>
#include <UnitsMethods_LengthUnit.hxx>
#include <XCAFApp_Application.hxx>
#include <XCAFDimTolObjects_DatumObject.hxx>
#include <XCAFDimTolObjects_DimensionObject.hxx>
#include <XCAFDimTolObjects_GeomToleranceObject.hxx>
#include <XCAFDoc_Datum.hxx>
#include <XCAFDoc_DimTolTool.hxx>
#include <XCAFDoc_Dimension.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <XCAFDoc_GeomTolerance.hxx>
#include <XCAFDoc_MaterialTool.hxx>
#include <XCAFDoc_ShapeTool.hxx>
#include <gp_Ax1.hxx>
#include <gp_Circ.hxx>
#include <gp_Cone.hxx>
#include <gp_Cylinder.hxx>
#include <gp_Pln.hxx>
#include <gp_Sphere.hxx>
#include <gp_Torus.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <map>
#include <string>
#include <utility>
#include <vector>

namespace {

// Bumped whenever the bridge changes what it writes. Together with the OCCT version it is the
// kernel version the worker fleet pins: two builds that tessellate differently must not
// produce derivatives that are cached as the same.
constexpr int BRIDGE_VERSION = 6;

const double PI = std::acos(-1.0);

// ---- output helpers --------------------------------------------------------------------

std::string jsonString(const std::string& text) {
  std::string out = "\"";
  for (const unsigned char c : text) {
    switch (c) {
      case '"': out += "\\\""; break;
      case '\\': out += "\\\\"; break;
      case '\n': out += "\\n"; break;
      case '\r': out += "\\r"; break;
      case '\t': out += "\\t"; break;
      default:
        if (c < 0x20) {
          char escaped[8];
          std::snprintf(escaped, sizeof escaped, "\\u%04x", c);
          out += escaped;
        } else {
          out += static_cast<char>(c);
        }
    }
  }
  return out + "\"";
}

// Seventeen significant digits round-trip a double exactly; a non-finite value is not JSON.
std::string number(double value) {
  if (!std::isfinite(value)) return "null";
  char buffer[32];
  std::snprintf(buffer, sizeof buffer, "%.17g", value);
  return buffer;
}

std::string xyz(const gp_XYZ& v) {
  return "[" + number(v.X()) + "," + number(v.Y()) + "," + number(v.Z()) + "]";
}

// Row-major 4x4, the last row always 0 0 0 1.
std::string matrix(const gp_Trsf& t) {
  std::string out = "[";
  for (int row = 1; row <= 3; ++row) {
    for (int col = 1; col <= 4; ++col) {
      out += number(t.Value(row, col)) + ",";
    }
  }
  return out + "0,0,0,1]";
}

bool writeFile(const std::string& path, const std::string& contents) {
  FILE* file = std::fopen(path.c_str(), "wb");
  if (file == nullptr) return false;
  const bool ok = std::fwrite(contents.data(), 1, contents.size(), file) == contents.size();
  return std::fclose(file) == 0 && ok;
}

int refuse(const std::string& detail) {
  std::fprintf(stderr, "{\"kind\":\"refused\",\"detail\":%s}\n", jsonString(detail).c_str());
  return 2;
}

// OCCT's default messenger prints transfer statistics to stdout, in colour. stdout is this
// program's answer to the worker, so nothing else may write to it.
void silenceOcct() {
  Message::DefaultMessenger()->RemovePrinters(STANDARD_TYPE(Message_PrinterOStream));
}

// ---- reading ---------------------------------------------------------------------------

occ::handle<TDocStd_Document> newDocument() {
  occ::handle<TDocStd_Document> doc;
  XCAFApp_Application::GetApplication()->NewDocument("MDTV-XCAF", doc);
  // Millimetres, whatever the file was written in: the readers scale into the document's
  // unit, so a part drawn in inches arrives converted rather than 25.4 times too small.
  XCAFDoc_DocumentTool::SetLengthUnit(doc, 1.0, UnitsMethods_LengthUnit_Millimeter);
  return doc;
}

// A header field as JSON: the text the file wrote, trimmed, or null when it wrote nothing.
std::string headerText(const occ::handle<TCollection_HAsciiString>& text) {
  if (text.IsNull()) return "null";
  const std::string value = text->ToCString();
  const auto first = value.find_first_not_of(" \t\r\n");
  if (first == std::string::npos) return "null";
  return jsonString(value.substr(first, value.find_last_not_of(" \t\r\n") - first + 1));
}

// A repeated header field as a JSON array, leaving out the entries the file left empty.
template <typename At>
std::string headerList(int count, At at) {
  std::string out = "[";
  for (int i = 1; i <= count; ++i) {
    const std::string value = headerText(at(i));
    if (value == "null") continue;
    if (out.size() > 1) out += ",";
    out += value;
  }
  return out + "]";
}

std::string headerFields(const std::string& fileName, const std::string& timeStamp,
                         const std::string& authors, const std::string& organizations,
                         const std::string& originatingSystem, const std::string& preprocessor,
                         const std::string& descriptions, const std::string& schemas) {
  return "\"file_name\":" + fileName + ",\"time_stamp\":" + timeStamp +
         ",\"authors\":" + authors + ",\"organizations\":" + organizations +
         ",\"originating_system\":" + originatingSystem + ",\"preprocessor\":" + preprocessor +
         ",\"descriptions\":" + descriptions + ",\"schemas\":" + schemas;
}

// `header` is left as it was when the file has no header to read.
bool readDocument(const std::string& path, const std::string& format,
                  const occ::handle<TDocStd_Document>& doc, std::string& header, std::string& why) {
  if (format == "step") {
    STEPCAFControl_Reader reader;
    reader.SetNameMode(true);
    reader.SetColorMode(false);
    reader.SetLayerMode(false);
    reader.SetMatMode(true);
    // Semantic PMI: the dimensions, tolerances and datums an AP242 file carries as data. What is
    // drawn in 3D beside them (the presentation) is not read into anything.
    reader.SetGDTMode(true);
    if (reader.ReadFile(path.c_str()) != IFSelect_RetDone) {
      why = "OCCT could not parse this file as STEP";
      return false;
    }
    const occ::handle<StepData_StepModel> model = reader.Reader().StepModel();
    if (!model.IsNull()) {
      const APIHeaderSection_MakeHeader made(model);
      if (made.IsDone()) {
        header = headerFields(
            headerText(made.Name()), headerText(made.TimeStamp()),
            headerList(made.NbAuthor(), [&](int i) { return made.AuthorValue(i); }),
            headerList(made.NbOrganization(), [&](int i) { return made.OrganizationValue(i); }),
            headerText(made.OriginatingSystem()), headerText(made.PreprocessorVersion()),
            headerList(made.NbDescription(), [&](int i) { return made.DescriptionValue(i); }),
            headerList(made.NbSchemaIdentifiers(),
                       [&](int i) { return made.SchemaIdentifiersValue(i); }));
      }
    }
    if (!reader.Transfer(doc)) {
      why = "the file parsed as STEP, but no shape could be transferred out of it";
      return false;
    }
    return true;
  }
  IGESCAFControl_Reader reader;
  reader.SetNameMode(true);
  if (reader.ReadFile(path.c_str()) != IFSelect_RetDone) {
    why = "OCCT could not parse this file as IGES";
    return false;
  }
  const occ::handle<IGESData_IGESModel> model = reader.IGESModel();
  if (!model.IsNull()) {
    const IGESData_GlobalSection& global = model->GlobalSection();
    header = headerFields(headerText(global.FileName()), headerText(global.Date()),
                          headerList(1, [&](int) { return global.AuthorName(); }),
                          headerList(1, [&](int) { return global.CompanyName(); }),
                          headerText(global.SystemId()), "null", "[]", "[]");
  }
  if (!reader.Transfer(doc)) {
    why = "the file parsed as IGES, but no shape could be transferred out of it";
    return false;
  }
  return true;
}

// Faces sewn into shells, and every shell that closes made a solid. For a file that transferred
// faces but no solids — IGES as most CAD tools write it, and the occasional STEP surface model —
// this is what lets a part that bounds a volume report one. A shell that stays open is left out
// and `solids` does not count it, so an open surface still reports no volume rather than a
// number integrated over a boundary that does not close.
//
// The tolerance is 1e-4 of the shape's diagonal: enough to close the seams a writer rounds,
// small enough not to fuse gaps the part really has.
TopoDS_Shape sewnSolids(const TopoDS_Shape& faces, int& solids) {
  Bnd_Box extent;
  BRepBndLib::Add(faces, extent);
  const double diagonal = extent.IsVoid() ? 0.0 : std::sqrt(extent.SquareExtent());
  BRepBuilderAPI_Sewing sewing(std::max(diagonal * 1e-4, 1e-6));
  sewing.Add(faces);
  sewing.Perform();
  TopoDS_Compound result;
  BRep_Builder builder;
  builder.MakeCompound(result);
  solids = 0;
  for (TopExp_Explorer shells(sewing.SewedShape(), TopAbs_SHELL); shells.More(); shells.Next()) {
    const TopoDS_Shell& shell = TopoDS::Shell(shells.Current());
    if (!BRep_Tool::IsClosed(shell)) continue;
    BRepBuilderAPI_MakeSolid make(shell);
    if (!make.IsDone()) continue;
    TopoDS_Solid solid = make.Solid();
    // A shell's faces may point inward, which integrates to a negative volume.
    BRepLib::OrientClosedSolid(solid);
    builder.Add(result, solid);
    ++solids;
  }
  return result;
}

std::string nameOf(const TDF_Label& label) {
  occ::handle<TDataStd_Name> name;
  if (label.FindAttribute(TDataStd_Name::GetID(), name)) {
    return TCollection_AsciiString(name->Get()).ToCString();
  }
  return "";
}

std::string entryOf(const TDF_Label& label) {
  TCollection_AsciiString entry;
  TDF_Tool::Entry(label, entry);
  return entry.ToCString();
}

// ---- structure.json --------------------------------------------------------------------

void writeNode(std::string& out, const TDF_Label& label, std::map<std::string, TDF_Label>& prototypes,
               int& parts) {
  TDF_Label shape = label;
  gp_Trsf local;
  if (XCAFDoc_ShapeTool::IsReference(label)) {
    XCAFDoc_ShapeTool::GetReferredShape(label, shape);
    local = XCAFDoc_ShapeTool::GetLocation(label).Transformation();
  }
  // The instance's own name when the file gives it one. OCCT names an unnamed instance after
  // the label it refers to (`=>[0:1:1:9]`), which is an address rather than a name, so that
  // falls through to the prototype's name exactly as an empty one does.
  std::string name = nameOf(label);
  if (name.empty() || name.rfind("=>", 0) == 0) name = nameOf(shape);
  out += "{\"name\":" + jsonString(name) + ",\"prototype\":" + jsonString(entryOf(shape)) +
         ",\"transform\":" + matrix(local);
  if (XCAFDoc_ShapeTool::IsAssembly(shape)) {
    NCollection_Sequence<TDF_Label> components;
    XCAFDoc_ShapeTool::GetComponents(shape, components);
    out += ",\"children\":[";
    for (int i = 1; i <= components.Length(); ++i) {
      if (i > 1) out += ",";
      writeNode(out, components.Value(i), prototypes, parts);
    }
    out += "]";
  } else {
    prototypes.emplace(entryOf(shape), shape);
    ++parts;
  }
  out += "}";
}

// ---- entities.json ---------------------------------------------------------------------

using ShapeMap = NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher>;

std::string surfaceEntity(int index, const TopoDS_Face& face) {
  const BRepAdaptor_Surface surface(face);
  const std::string head = "{\"face\":" + std::to_string(index) + ",\"type\":";
  switch (surface.GetType()) {
    case GeomAbs_Plane: {
      const gp_Pln plane = surface.Plane();
      return head + "\"plane\",\"origin\":" + xyz(plane.Location().XYZ()) +
             ",\"normal\":" + xyz(plane.Axis().Direction().XYZ()) + "}";
    }
    case GeomAbs_Cylinder: {
      const gp_Cylinder cylinder = surface.Cylinder();
      return head + "\"cylinder\",\"radius\":" + number(cylinder.Radius()) +
             ",\"origin\":" + xyz(cylinder.Location().XYZ()) +
             ",\"axis\":" + xyz(cylinder.Axis().Direction().XYZ()) + "}";
    }
    case GeomAbs_Cone: {
      const gp_Cone cone = surface.Cone();
      return head + "\"cone\",\"ref_radius\":" + number(cone.RefRadius()) +
             ",\"semi_angle_rad\":" + number(cone.SemiAngle()) +
             ",\"origin\":" + xyz(cone.Location().XYZ()) +
             ",\"axis\":" + xyz(cone.Axis().Direction().XYZ()) + "}";
    }
    case GeomAbs_Sphere: {
      const gp_Sphere sphere = surface.Sphere();
      return head + "\"sphere\",\"radius\":" + number(sphere.Radius()) +
             ",\"center\":" + xyz(sphere.Location().XYZ()) + "}";
    }
    case GeomAbs_Torus: {
      const gp_Torus torus = surface.Torus();
      return head + "\"torus\",\"major_radius\":" + number(torus.MajorRadius()) +
             ",\"minor_radius\":" + number(torus.MinorRadius()) +
             ",\"origin\":" + xyz(torus.Axis().Location().XYZ()) +
             ",\"axis\":" + xyz(torus.Axis().Direction().XYZ()) + "}";
    }
    default:
      return "";
  }
}

void writeEntities(std::string& out, const std::string& prototype, const TopoDS_Shape& shape) {
  ShapeMap faces;
  ShapeMap edges;
  TopExp::MapShapes(shape, TopAbs_FACE, faces);
  TopExp::MapShapes(shape, TopAbs_EDGE, edges);
  out += "{\"prototype\":" + jsonString(prototype) + ",\"faces\":[";
  bool first = true;
  for (int i = 1; i <= faces.Extent(); ++i) {
    const std::string entity = surfaceEntity(i, TopoDS::Face(faces.FindKey(i)));
    if (entity.empty()) continue;
    if (!first) out += ",";
    out += entity;
    first = false;
  }
  out += "],\"circles\":[";
  first = true;
  for (int i = 1; i <= edges.Extent(); ++i) {
    const TopoDS_Edge edge = TopoDS::Edge(edges.FindKey(i));
    // A degenerated edge — the pole of a sphere, the apex of a cone — has no 3D curve at all.
    if (BRep_Tool::Degenerated(edge)) continue;
    const BRepAdaptor_Curve curve(edge);
    if (curve.GetType() != GeomAbs_Circle) continue;
    const gp_Circ circle = curve.Circle();
    if (!first) out += ",";
    out += "{\"edge\":" + std::to_string(i) + ",\"radius\":" + number(circle.Radius()) +
           ",\"center\":" + xyz(circle.Location().XYZ()) +
           ",\"normal\":" + xyz(circle.Axis().Direction().XYZ()) + "}";
    first = false;
  }
  out += "]}";
}

// ---- mesh.stl --------------------------------------------------------------------------

void putU32(std::string& out, std::uint32_t value) {
  for (int i = 0; i < 4; ++i) out += static_cast<char>((value >> (8 * i)) & 0xff);
}

void putF32(std::string& out, double value) {
  const float f = static_cast<float>(value);
  std::uint32_t bits = 0;
  std::memcpy(&bits, &f, sizeof bits);
  putU32(out, bits);
}

// One face's triangles, placed by the location the face carries.
void meshFace(std::string& body, const TopoDS_Face& face, std::uint32_t& count) {
  TopLoc_Location location;
  const occ::handle<Poly_Triangulation>& triangulation = BRep_Tool::Triangulation(face, location);
  if (triangulation.IsNull()) return;
  const gp_Trsf placement = location.Transformation();
  const bool reversed = face.Orientation() == TopAbs_REVERSED;
  for (int t = 1; t <= triangulation->NbTriangles(); ++t) {
    int n1 = 0, n2 = 0, n3 = 0;
    triangulation->Triangle(t).Get(n1, n2, n3);
    if (reversed) std::swap(n2, n3);
    for (int i = 0; i < 3; ++i) putF32(body, 0.0);  // normal: readers recompute it
    for (const int node : {n1, n2, n3}) {
      const gp_Pnt p = triangulation->Node(node).Transformed(placement);
      putF32(body, p.X());
      putF32(body, p.Y());
      putF32(body, p.Z());
    }
    body += std::string(2, '\0');
    ++count;
  }
}

// Every placed part's triangles, walked in `writeNode`'s order: roots, then components, depth
// first. So `parts[n]` counts the triangles of structure.json's n-th leaf, and they are the next
// `parts[n]` triangles of mesh.stl. The faces were meshed once, on the whole shape; a leaf only
// places its prototype's faces where the tree puts them.
void meshNode(std::string& body, const TDF_Label& label, const gp_Trsf& parent,
              std::vector<std::uint32_t>& parts, std::uint32_t& count) {
  TDF_Label shape = label;
  gp_Trsf placement = parent;
  if (XCAFDoc_ShapeTool::IsReference(label)) {
    XCAFDoc_ShapeTool::GetReferredShape(label, shape);
    placement = parent * XCAFDoc_ShapeTool::GetLocation(label).Transformation();
  }
  if (XCAFDoc_ShapeTool::IsAssembly(shape)) {
    NCollection_Sequence<TDF_Label> components;
    XCAFDoc_ShapeTool::GetComponents(shape, components);
    for (int i = 1; i <= components.Length(); ++i) {
      meshNode(body, components.Value(i), placement, parts, count);
    }
    return;
  }
  const std::uint32_t before = count;
  const TopoDS_Shape placed =
      XCAFDoc_ShapeTool::GetShape(shape).Moved(TopLoc_Location(placement));
  for (TopExp_Explorer explorer(placed, TopAbs_FACE); explorer.More(); explorer.Next()) {
    meshFace(body, TopoDS::Face(explorer.Current()), count);
  }
  parts.push_back(count - before);
}

std::uint32_t writeMesh(const std::string& path, const NCollection_Sequence<TDF_Label>& roots,
                        std::vector<std::uint32_t>& parts, bool& ok) {
  std::string body;
  std::uint32_t count = 0;
  for (int i = 1; i <= roots.Length(); ++i) meshNode(body, roots.Value(i), gp_Trsf(), parts, count);
  std::string file(80, ' ');
  const char header[] = "occt-bridge mesh.stl";
  std::memcpy(&file[0], header, sizeof header - 1);
  putU32(file, count);
  ok = writeFile(path, file + body);
  return count;
}

// ---- pmi.json --------------------------------------------------------------------------

std::string dimensionType(XCAFDimTolObjects_DimensionType type) {
  switch (type) {
    case XCAFDimTolObjects_DimensionType_Size_Diameter: return "diameter";
    case XCAFDimTolObjects_DimensionType_Size_Radius: return "radius";
    case XCAFDimTolObjects_DimensionType_Size_SphericalDiameter: return "spherical_diameter";
    case XCAFDimTolObjects_DimensionType_Size_SphericalRadius: return "spherical_radius";
    case XCAFDimTolObjects_DimensionType_Size_Thickness: return "thickness";
    case XCAFDimTolObjects_DimensionType_Size_Angular:
    case XCAFDimTolObjects_DimensionType_Location_Angular: return "angle";
    case XCAFDimTolObjects_DimensionType_Location_LinearDistance:
    case XCAFDimTolObjects_DimensionType_Location_LinearDistance_FromCenterToOuter:
    case XCAFDimTolObjects_DimensionType_Location_LinearDistance_FromCenterToInner:
    case XCAFDimTolObjects_DimensionType_Location_LinearDistance_FromOuterToCenter:
    case XCAFDimTolObjects_DimensionType_Location_LinearDistance_FromOuterToOuter:
    case XCAFDimTolObjects_DimensionType_Location_LinearDistance_FromOuterToInner:
    case XCAFDimTolObjects_DimensionType_Location_LinearDistance_FromInnerToCenter:
    case XCAFDimTolObjects_DimensionType_Location_LinearDistance_FromInnerToOuter:
    case XCAFDimTolObjects_DimensionType_Location_LinearDistance_FromInnerToInner: return "distance";
    default: return "other";
  }
}

std::string toleranceType(XCAFDimTolObjects_GeomToleranceType type) {
  switch (type) {
    case XCAFDimTolObjects_GeomToleranceType_Angularity: return "angularity";
    case XCAFDimTolObjects_GeomToleranceType_CircularRunout: return "circular_runout";
    case XCAFDimTolObjects_GeomToleranceType_CircularityOrRoundness: return "circularity";
    case XCAFDimTolObjects_GeomToleranceType_Coaxiality: return "coaxiality";
    case XCAFDimTolObjects_GeomToleranceType_Concentricity: return "concentricity";
    case XCAFDimTolObjects_GeomToleranceType_Cylindricity: return "cylindricity";
    case XCAFDimTolObjects_GeomToleranceType_Flatness: return "flatness";
    case XCAFDimTolObjects_GeomToleranceType_Parallelism: return "parallelism";
    case XCAFDimTolObjects_GeomToleranceType_Perpendicularity: return "perpendicularity";
    case XCAFDimTolObjects_GeomToleranceType_Position: return "position";
    case XCAFDimTolObjects_GeomToleranceType_ProfileOfLine: return "profile_of_line";
    case XCAFDimTolObjects_GeomToleranceType_ProfileOfSurface: return "profile_of_surface";
    case XCAFDimTolObjects_GeomToleranceType_Straightness: return "straightness";
    case XCAFDimTolObjects_GeomToleranceType_Symmetry: return "symmetry";
    case XCAFDimTolObjects_GeomToleranceType_TotalRunout: return "total_runout";
    default: return "other";
  }
}

// The faces an annotation's shape labels name, as `entities.json` names faces: the prototype the
// face belongs to and its 1-based index in that prototype's face map. A label that is a whole
// part rather than one of its faces names the part with no face.
std::string faceRefs(const NCollection_Sequence<TDF_Label>& labels) {
  std::string out = "[";
  for (int i = 1; i <= labels.Length(); ++i) {
    const TDF_Label& label = labels.Value(i);
    const TDF_Label owner = label.Father();
    const TopoDS_Shape shape = XCAFDoc_ShapeTool::GetShape(label);
    std::string ref;
    if (shape.ShapeType() == TopAbs_FACE && XCAFDoc_ShapeTool::IsShape(owner)) {
      ShapeMap faces;
      TopExp::MapShapes(XCAFDoc_ShapeTool::GetShape(owner), TopAbs_FACE, faces);
      const int index = faces.FindIndex(shape);
      if (index > 0) {
        ref = "{\"prototype\":" + jsonString(entryOf(owner)) + ",\"face\":" + std::to_string(index) + "}";
      }
    }
    if (ref.empty()) ref = "{\"prototype\":" + jsonString(entryOf(label)) + ",\"face\":null}";
    if (out.size() > 1) out += ",";
    out += ref;
  }
  return out + "]";
}

// A datum's letter. The reader keeps it on the datum attribute, and on the datum's object when the
// file gave one; either may be empty.
std::string datumName(const occ::handle<XCAFDoc_Datum>& datum) {
  if (!datum->GetObject().IsNull()) {
    const std::string fromObject = headerText(datum->GetObject()->GetName());
    if (fromObject != "null") return fromObject;
  }
  return headerText(datum->GetName());
}

std::string writePmi(const occ::handle<TDocStd_Document>& doc) {
  const occ::handle<XCAFDoc_DimTolTool> tool = XCAFDoc_DocumentTool::DimTolTool(doc->Main());
  std::string out = "{\"dimensions\":[";
  NCollection_Sequence<TDF_Label> labels;
  tool->GetDimensionLabels(labels);
  bool first = true;
  for (int i = 1; i <= labels.Length(); ++i) {
    occ::handle<XCAFDoc_Dimension> attribute;
    if (!labels.Value(i).FindAttribute(XCAFDoc_Dimension::GetID(), attribute)) continue;
    const occ::handle<XCAFDimTolObjects_DimensionObject> dimension = attribute->GetObject();
    if (dimension.IsNull()) continue;
    NCollection_Sequence<TDF_Label> shapes, second;
    XCAFDoc_DimTolTool::GetRefShapeLabel(labels.Value(i), shapes, second);
    shapes.Append(second);
    const bool bounded = dimension->IsDimWithPlusMinusTolerance();
    if (!first) out += ",";
    out += "{\"type\":" + jsonString(dimensionType(dimension->GetType())) +
           ",\"value\":" + number(dimension->GetValue()) +
           ",\"upper\":" + (bounded ? number(dimension->GetUpperTolValue()) : "null") +
           ",\"lower\":" + (bounded ? number(dimension->GetLowerTolValue()) : "null") +
           ",\"faces\":" + faceRefs(shapes) + "}";
    first = false;
  }
  out += "],\"tolerances\":[";
  labels.Clear();
  tool->GetGeomToleranceLabels(labels);
  first = true;
  for (int i = 1; i <= labels.Length(); ++i) {
    occ::handle<XCAFDoc_GeomTolerance> attribute;
    if (!labels.Value(i).FindAttribute(XCAFDoc_GeomTolerance::GetID(), attribute)) continue;
    const occ::handle<XCAFDimTolObjects_GeomToleranceObject> tolerance = attribute->GetObject();
    if (tolerance.IsNull()) continue;
    NCollection_Sequence<TDF_Label> shapes, second;
    XCAFDoc_DimTolTool::GetRefShapeLabel(labels.Value(i), shapes, second);
    NCollection_Sequence<TDF_Label> datums;
    XCAFDoc_DimTolTool::GetDatumOfTolerLabels(labels.Value(i), datums);
    std::string names = "[";
    for (int d = 1; d <= datums.Length(); ++d) {
      occ::handle<XCAFDoc_Datum> datum;
      if (!datums.Value(d).FindAttribute(XCAFDoc_Datum::GetID(), datum)) continue;
      const std::string name = datumName(datum);
      if (name == "null") continue;
      if (names.size() > 1) names += ",";
      names += name;
    }
    names += "]";
    if (!first) out += ",";
    out += "{\"type\":" + jsonString(toleranceType(tolerance->GetType())) +
           ",\"value\":" + number(tolerance->GetValue()) + ",\"datums\":" + names +
           ",\"faces\":" + faceRefs(shapes) + "}";
    first = false;
  }
  out += "],\"datums\":[";
  labels.Clear();
  tool->GetDatumLabels(labels);
  first = true;
  for (int i = 1; i <= labels.Length(); ++i) {
    occ::handle<XCAFDoc_Datum> datum;
    if (!labels.Value(i).FindAttribute(XCAFDoc_Datum::GetID(), datum)) continue;
    const std::string name = datumName(datum);
    if (name == "null") continue;
    NCollection_Sequence<TDF_Label> shapes, second;
    XCAFDoc_DimTolTool::GetRefShapeLabel(labels.Value(i), shapes, second);
    if (!first) out += ",";
    out += "{\"name\":" + name + ",\"faces\":" + faceRefs(shapes) + "}";
    first = false;
  }
  return out + "]}\n";
}

// ---- convert ---------------------------------------------------------------------------

int convert(const std::string& in, const std::string& format, const std::string& outDir,
            double deflection) {
  if (format != "step" && format != "iges") {
    return refuse("occt-bridge reads step and iges, not " + format);
  }
  const occ::handle<TDocStd_Document> doc = newDocument();
  std::string header = headerFields("null", "null", "[]", "[]", "null", "null", "[]", "[]");
  std::string why;
  if (!readDocument(in, format, doc, header, why)) return refuse(why);

  // The materials the file defines, by name. STEP carries them when the exporter wrote any;
  // IGES has none to carry.
  const occ::handle<XCAFDoc_MaterialTool> materialTool =
      XCAFDoc_DocumentTool::MaterialTool(doc->Main());
  NCollection_Sequence<TDF_Label> materialLabels;
  materialTool->GetMaterialLabels(materialLabels);
  std::string materials = "[";
  for (int i = 1; i <= materialLabels.Length(); ++i) {
    occ::handle<TCollection_HAsciiString> name, description, densityName, densityType;
    double density = 0.0;
    if (!XCAFDoc_MaterialTool::GetMaterial(materialLabels.Value(i), name, description, density,
                                           densityName, densityType)) {
      continue;
    }
    const std::string value = headerText(name);
    if (value == "null") continue;
    if (materials.size() > 1) materials += ",";
    materials += value;
  }
  materials += "]";

  const occ::handle<XCAFDoc_ShapeTool> tool = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
  NCollection_Sequence<TDF_Label> roots;
  tool->GetFreeShapes(roots);
  if (roots.Length() == 0) return refuse("the file holds no shapes");

  std::map<std::string, TDF_Label> prototypes;
  int parts = 0;
  std::string structure = "{\"units\":\"mm\",\"roots\":[";
  for (int i = 1; i <= roots.Length(); ++i) {
    if (i > 1) structure += ",";
    writeNode(structure, roots.Value(i), prototypes, parts);
  }
  structure += "],\"parts\":" + std::to_string(parts) +
               ",\"prototypes\":" + std::to_string(prototypes.size()) + "}\n";

  std::string entities = "{\"units\":\"mm\",\"prototypes\":[";
  bool first = true;
  for (const auto& [entry, label] : prototypes) {
    if (!first) entities += ",";
    writeEntities(entities, entry, XCAFDoc_ShapeTool::GetShape(label));
    first = false;
  }
  entities += "]}\n";

  const TopoDS_Shape whole = tool->GetOneShape();
  int solids = 0;
  for (TopExp_Explorer explorer(whole, TopAbs_SOLID); explorer.More(); explorer.Next()) ++solids;
  // No solids came across: sew the faces and measure whatever closes. Area and the box stay the
  // faces' own, which is what the file holds either way.
  const TopoDS_Shape solidsToMeasure = solids > 0 ? whole : sewnSolids(whole, solids);
  GProp_GProps volume;
  BRepGProp::VolumeProperties(solidsToMeasure, volume);
  GProp_GProps area;
  BRepGProp::SurfaceProperties(whole, area);
  // From the B-rep, not the mesh: `useTriangulation` off, so the box is the geometry's.
  Bnd_Box box;
  BRepBndLib::AddOptimal(whole, box, false, false);
  // Every face and edge of the placed shape, analytic or not: what the revision diff counts. Placed, so
  // an assembly counts each instance of a prototype's faces.
  ShapeMap faces;
  ShapeMap edges;
  TopExp::MapShapes(whole, TopAbs_FACE, faces);
  TopExp::MapShapes(whole, TopAbs_EDGE, edges);
  std::string measurements = "{\"units\":\"mm\",\"solids\":" + std::to_string(solids) +
                             ",\"faces\":" + std::to_string(faces.Extent()) +
                             ",\"edges\":" + std::to_string(edges.Extent()) +
                             ",\"volume_mm3\":" + (solids > 0 ? number(volume.Mass()) : "null") +
                             ",\"surface_area_mm2\":" + number(area.Mass());
  if (box.IsVoid()) {
    measurements += ",\"bbox_min\":null,\"bbox_max\":null,\"bbox_mm\":null}\n";
  } else {
    const gp_XYZ lo = box.CornerMin().XYZ();
    const gp_XYZ hi = box.CornerMax().XYZ();
    measurements += ",\"bbox_min\":" + xyz(lo) + ",\"bbox_max\":" + xyz(hi) +
                    ",\"bbox_mm\":" + xyz(hi - lo) + "}\n";
  }

  const BRepMesh_IncrementalMesh mesher(whole, deflection, false, 0.5, true);
  bool meshWritten = false;
  std::vector<std::uint32_t> partTriangles;
  const std::uint32_t triangles =
      writeMesh(outDir + "/mesh.stl", roots, partTriangles, meshWritten);
  std::string partsJson = "[";
  for (std::size_t i = 0; i < partTriangles.size(); ++i) {
    if (i > 0) partsJson += ",";
    partsJson += std::to_string(partTriangles[i]);
  }
  partsJson += "]\n";

  if (!meshWritten || !writeFile(outDir + "/parts.json", partsJson) ||
      !writeFile(outDir + "/pmi.json", writePmi(doc)) ||
      !writeFile(outDir + "/structure.json", structure) ||
      !writeFile(outDir + "/entities.json", entities) ||
      !writeFile(outDir + "/measurements.json", measurements) ||
      !writeFile(outDir + "/header.json", "{" + header + ",\"materials\":" + materials + "}\n")) {
    std::fprintf(stderr, "occt-bridge: could not write into %s\n", outDir.c_str());
    return 1;
  }
  std::printf("{\"parts\":%d,\"prototypes\":%zu,\"solids\":%d,\"triangles\":%u}\n", parts,
              prototypes.size(), solids, triangles);
  return 0;
}

// ---- generate-fixtures -----------------------------------------------------------------

gp_Trsf at(double x, double y, double z) {
  gp_Trsf t;
  t.SetTranslation(gp_Vec(x, y, z));
  return t;
}

TopoDS_Shape box(double dx, double dy, double dz) { return BRepPrimAPI_MakeBox(dx, dy, dz).Shape(); }

TopoDS_Shape cylinder(double radius, double height) {
  return BRepPrimAPI_MakeCylinder(radius, height).Shape();
}

TopoDS_Shape placed(const TopoDS_Shape& shape, const gp_Trsf& t) {
  return BRepBuilderAPI_Transform(shape, t, true).Shape();
}

TopoDS_Shape fused(const TopoDS_Shape& a, const TopoDS_Shape& b) { return BRepAlgoAPI_Fuse(a, b).Shape(); }

TopoDS_Shape cut(const TopoDS_Shape& a, const TopoDS_Shape& b) { return BRepAlgoAPI_Cut(a, b).Shape(); }

bool writeStep(const occ::handle<TDocStd_Document>& doc, const std::string& path,
               UnitsMethods_LengthUnit unit) {
  DESTEP_Parameters params;
  params.WriteSchema = DESTEP_Parameters::WriteMode_StepSchema_AP242DIS;
  params.WriteUnit = unit;
  STEPCAFControl_Writer writer;
  writer.SetMaterialMode(true);
  writer.SetDimTolMode(true);
  return writer.Transfer(doc, params) && writer.Write(path.c_str()) == IFSelect_RetDone;
}

// A welding and inspection fixture: a plate on four levelling feet, twelve bracket stations,
// a rail of V-blocks and a rack of stop pins. Ten prototypes would do; the point is 200 placed
// parts through three levels of assembly, which is the Phase 0 exit test's shape.
int generateFixtures(const std::string& dir) {
  const occ::handle<TDocStd_Document> doc = newDocument();
  const occ::handle<XCAFDoc_ShapeTool> tool = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
  const auto prototype = [&](const TopoDS_Shape& shape, const char* name) {
    const TDF_Label label = tool->AddShape(shape, false);
    TDataStd_Name::Set(label, name);
    return label;
  };
  const auto assembly = [&](const char* name) {
    const TDF_Label label = tool->NewShape();
    TDataStd_Name::Set(label, name);
    return label;
  };
  int placedParts = 0;
  const auto place = [&](const TDF_Label& into, const TDF_Label& what, double x, double y, double z,
                         bool leaf = true) {
    tool->AddComponent(into, what, TopLoc_Location(at(x, y, z)));
    if (leaf) ++placedParts;
  };

  BRep_Builder builder;
  TopoDS_Compound holes;
  builder.MakeCompound(holes);
  for (const auto& [x, y] : {std::pair{15.0, 15.0}, {285.0, 15.0}, {15.0, 185.0}, {285.0, 185.0}}) {
    builder.Add(holes, placed(cylinder(5.0, 20.0), at(x, y, 0.0)));
  }
  const TDF_Label plate = prototype(cut(box(300.0, 200.0, 20.0), holes), "fixture-plate-300x200x20-lp-9001-00");
  const TDF_Label foot = prototype(fused(cylinder(15.0, 8.0), placed(cylinder(5.0, 32.0), at(0, 0, 8.0))),
                                   "leveling-foot-m10x40-lp-9008-00");
  const TDF_Label bracket = prototype(fused(box(60.0, 40.0, 8.0), box(8.0, 40.0, 60.0)),
                                      "angle-bracket-60x60x40-lp-9004-00");
  const TDF_Label screw = prototype(fused(cylinder(3.0, 25.0), placed(cylinder(5.0, 6.0), at(0, 0, 25.0))),
                                    "socket-cap-screw-m6x25-lp-9003-00");
  const TDF_Label dowel = prototype(cylinder(4.0, 30.0), "locating-dowel-d8x30-lp-9002-00");
  const TDF_Label clamp = prototype(box(40.0, 30.0, 12.0), "toggle-clamp-base-40x30x12-lp-9005-00");
  gp_Trsf tilt;
  tilt.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 1, 0)), PI / 4.0);
  const TopoDS_Shape groove = placed(placed(placed(box(30.0, 60.0, 30.0), at(-15.0, -10.0, -15.0)), tilt), at(25.0, 0.0, 30.0));
  const TDF_Label vblock = prototype(cut(box(50.0, 40.0, 30.0), groove), "v-block-50x40x30-lp-9006-00");
  const TDF_Label pin = prototype(cylinder(5.0, 20.0), "stop-pin-d10x20-lp-9007-00");

  const TDF_Label station = assembly("bracket-station-lp-9100-00");
  place(station, bracket, 0, 0, 0);
  for (const auto& [x, y] : {std::pair{10.0, 8.0}, {10.0, 20.0}, {10.0, 32.0}, {30.0, 8.0},
                             {30.0, 32.0}, {50.0, 8.0}, {50.0, 20.0}, {50.0, 32.0}}) {
    place(station, screw, x, y, -17.0);
  }
  place(station, dowel, 20.0, 20.0, -22.0);
  place(station, dowel, 40.0, 20.0, -22.0);
  place(station, clamp, 14.0, 5.0, 14.0);
  const int perStation = placedParts;
  placedParts = 0;

  const TDF_Label rail = assembly("v-block-rail-lp-9200-00");
  for (int i = 0; i < 6; ++i) place(rail, vblock, i * 52.0, 0, 0);
  const TDF_Label rack = assembly("stop-pin-rack-lp-9300-00");
  for (int row = 0; row < 3; ++row) {
    for (int col = 0; col < 15; ++col) place(rack, pin, col * 20.0, row * 20.0, 0);
  }
  // The rail and the rack count their parts where the top assembly places them, below — as the
  // stations do — so the placements inside them are not counted twice.
  placedParts = 0;

  const TDF_Label top = assembly("fixture-plate-assembly-lp-9000-00");
  place(top, plate, 0, 0, 0);
  for (const auto& [x, y] : {std::pair{15.0, 15.0}, {285.0, 15.0}, {15.0, 185.0}, {285.0, 185.0}}) {
    place(top, foot, x, y, -40.0);
  }
  for (int row = 0; row < 3; ++row) {
    for (int col = 0; col < 4; ++col) {
      place(top, station, 10.0 + col * 72.0, 10.0 + row * 62.0, 20.0, false);
      placedParts += perStation;
    }
  }
  place(top, rail, 0, 215.0, 0, false);
  placedParts += 6;
  place(top, rack, 0, 270.0, 0, false);
  placedParts += 45;
  tool->UpdateAssemblies();
  if (placedParts != 200) {
    std::fprintf(stderr, "generate-fixtures: built %d placed parts, not 200\n", placedParts);
    return 1;
  }

  const occ::handle<TDocStd_Document> single = newDocument();
  const occ::handle<XCAFDoc_ShapeTool> singleTool = XCAFDoc_DocumentTool::ShapeTool(single->Main());
  const TDF_Label cylinderLabel = singleTool->AddShape(cylinder(11.0, 30.0), false);
  TDataStd_Name::Set(cylinderLabel, "cylinder-d22-lp-9010-00");
  // A material, so one fixture carries what `header.json` reads and the material facet counts.
  XCAFDoc_DocumentTool::MaterialTool(single->Main())
      ->SetMaterial(cylinderLabel, new TCollection_HAsciiString("Stainless steel 1.4301"),
                    new TCollection_HAsciiString("X5CrNi18-10"), 7.9,
                    new TCollection_HAsciiString("density"),
                    new TCollection_HAsciiString("POSITIVE_RATIO_MEASURE"));

  // The same cylinder with the PMI a drawing would give it, written as AP242 semantic data: a
  // diameter of 22 mm +0.05/-0 on the cylindrical face, datum A on the base, flatness 0.02 mm on
  // the top face and perpendicularity 0.05 mm of the cylindrical face to A.
  const occ::handle<TDocStd_Document> pmi = newDocument();
  const occ::handle<XCAFDoc_ShapeTool> pmiTool = XCAFDoc_DocumentTool::ShapeTool(pmi->Main());
  const TopoDS_Shape pmiShape = cylinder(11.0, 30.0);
  const TDF_Label pmiLabel = pmiTool->AddShape(pmiShape, false);
  TDataStd_Name::Set(pmiLabel, "cylinder-d22-pmi-lp-9012-00");
  TDF_Label pmiSide, pmiBase, pmiTop;
  for (TopExp_Explorer explorer(pmiShape, TopAbs_FACE); explorer.More(); explorer.Next()) {
    const TopoDS_Face face = TopoDS::Face(explorer.Current());
    const BRepAdaptor_Surface surface(face);
    const TDF_Label label = pmiTool->AddSubShape(pmiLabel, face);
    if (surface.GetType() == GeomAbs_Cylinder) {
      pmiSide = label;
    } else if (surface.GetType() == GeomAbs_Plane) {
      (surface.Plane().Location().Z() < 15.0 ? pmiBase : pmiTop) = label;
    }
  }
  const occ::handle<XCAFDoc_DimTolTool> dimTol = XCAFDoc_DocumentTool::DimTolTool(pmi->Main());
  const TDF_Label diameterLabel = dimTol->AddDimension();
  dimTol->SetDimension(pmiSide, diameterLabel);
  const occ::handle<XCAFDimTolObjects_DimensionObject> diameter =
      new XCAFDimTolObjects_DimensionObject();
  diameter->SetType(XCAFDimTolObjects_DimensionType_Size_Diameter);
  diameter->SetValue(22.0);
  diameter->SetUpperTolValue(0.05);
  diameter->SetLowerTolValue(0.0);
  XCAFDoc_Dimension::Set(diameterLabel)->SetObject(diameter);

  const TDF_Label datumLabel = dimTol->AddDatum();
  NCollection_Sequence<TDF_Label> datumFaces;
  datumFaces.Append(pmiBase);
  dimTol->SetDatum(datumFaces, datumLabel);
  const occ::handle<XCAFDimTolObjects_DatumObject> datumA = new XCAFDimTolObjects_DatumObject();
  datumA->SetName(new TCollection_HAsciiString("A"));
  // First in the datum reference frame of the tolerance below. A datum at position 0 has no place
  // in one, and the writer then drops the tolerance that refers to it.
  datumA->SetPosition(1);
  XCAFDoc_Datum::Set(datumLabel)->SetObject(datumA);

  const TDF_Label flatnessLabel = dimTol->AddGeomTolerance();
  dimTol->SetGeomTolerance(pmiTop, flatnessLabel);
  const occ::handle<XCAFDimTolObjects_GeomToleranceObject> flatness =
      new XCAFDimTolObjects_GeomToleranceObject();
  flatness->SetType(XCAFDimTolObjects_GeomToleranceType_Flatness);
  flatness->SetValue(0.02);
  XCAFDoc_GeomTolerance::Set(flatnessLabel)->SetObject(flatness);

  const TDF_Label squareLabel = dimTol->AddGeomTolerance();
  dimTol->SetGeomTolerance(pmiSide, squareLabel);
  const occ::handle<XCAFDimTolObjects_GeomToleranceObject> square =
      new XCAFDimTolObjects_GeomToleranceObject();
  square->SetType(XCAFDimTolObjects_GeomToleranceType_Perpendicularity);
  square->SetValue(0.05);
  XCAFDoc_GeomTolerance::Set(squareLabel)->SetObject(square);
  dimTol->SetDatumToGeomTol(datumLabel, squareLabel);

  const occ::handle<TDocStd_Document> iges = newDocument();
  const occ::handle<XCAFDoc_ShapeTool> igesTool = XCAFDoc_DocumentTool::ShapeTool(iges->Main());
  TDataStd_Name::Set(igesTool->AddShape(fused(box(60.0, 40.0, 8.0), box(8.0, 40.0, 60.0)), false),
                     "angle-bracket-60x60x40-lp-9004-00");
  IGESCAFControl_Writer igesWriter;

  const bool ok =
      writeStep(doc, dir + "/fixture-plate-assembly-lp-9000-00.step", UnitsMethods_LengthUnit_Millimeter) &&
      writeStep(single, dir + "/cylinder-d22-lp-9010-00.step", UnitsMethods_LengthUnit_Millimeter) &&
      writeStep(single, dir + "/cylinder-d22-inch-units-lp-9011-00.step", UnitsMethods_LengthUnit_Inch) &&
      writeStep(pmi, dir + "/cylinder-d22-pmi-lp-9012-00.step", UnitsMethods_LengthUnit_Millimeter) &&
      igesWriter.Transfer(iges) && igesWriter.Write((dir + "/angle-bracket-60x60x40-lp-9004-00.igs").c_str());
  if (!ok) {
    std::fprintf(stderr, "generate-fixtures: could not write into %s\n", dir.c_str());
    return 1;
  }
  std::printf("{\"placed_parts\":%d}\n", placedParts);
  return 0;
}

// ---- selftest and version --------------------------------------------------------------

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
  const occ::handle<TDocStd_Document> written = newDocument();
  XCAFDoc_DocumentTool::ShapeTool(written->Main())->AddShape(cylinder(radius, height));
  const std::string path = std::string(dir) + "/occt-bridge-selftest-cylinder-d22.step";
  if (!writeStep(written, path, UnitsMethods_LengthUnit_Millimeter)) {
    std::fprintf(stderr, "selftest: could not write %s\n", path.c_str());
    return 1;
  }
  const occ::handle<TDocStd_Document> read = newDocument();
  std::string header;
  std::string why;
  if (!readDocument(path, "step", read, header, why)) {
    std::fprintf(stderr, "selftest: %s\n", why.c_str());
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
  std::fprintf(stderr, "usage: occt-bridge version\n"
                       "       occt-bridge selftest <scratch-dir>\n"
                       "       occt-bridge convert --in <file> --format step|iges --out <dir> [--deflection <mm>]\n"
                       "       occt-bridge generate-fixtures <dir>\n");
  return 64;
}

}  // namespace

int main(int argc, char** argv) {
  silenceOcct();
  const std::string command = argc >= 2 ? argv[1] : "";
  try {
    if (command == "version") return version();
    if (command == "selftest" && argc >= 3) return selftest(argv[2]);
    if (command == "generate-fixtures" && argc >= 3) return generateFixtures(argv[2]);
    if (command == "convert") {
      std::string in, format, out;
      double deflection = 0.1;
      for (int i = 2; i + 1 < argc; i += 2) {
        const std::string flag = argv[i];
        if (flag == "--in") in = argv[i + 1];
        else if (flag == "--format") format = argv[i + 1];
        else if (flag == "--out") out = argv[i + 1];
        else if (flag == "--deflection") deflection = std::atof(argv[i + 1]);
        else return usage();
      }
      if (in.empty() || format.empty() || out.empty() || !(deflection > 0.0)) return usage();
      return convert(in, format, out, deflection);
    }
  } catch (const Standard_Failure& failure) {
    // OCCT raises on geometry it cannot handle. For `convert` that is a refusal of this file,
    // not a crash of the bridge: another attempt reads the same bytes and raises again.
    if (command == "convert") return refuse(std::string("OCCT raised ") + failure.what());
    std::fprintf(stderr, "occt-bridge: OCCT raised %s\n", failure.what());
    return 1;
  }
  return usage();
}
