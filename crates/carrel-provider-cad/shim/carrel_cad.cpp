// The part of carrel-provider-cad that needs OpenCascade: it reads a STEP or IGES file,
// meshes it coarsely, and hands back triangles, the number of solids and their volume.
// docs/design/models-3d.md, "Drawing STEP and IGES".

#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <vector>

#include <BRepBndLib.hxx>
#include <BRepGProp.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRep_Tool.hxx>
#include <Bnd_Box.hxx>
#include <GProp_GProps.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <IGESControl_Reader.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Poly_Triangulation.hxx>
#include <STEPControl_Reader.hxx>
#include <Standard_Failure.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Shape.hxx>

extern "C" {

// A meshed model. The arrays are owned by the shim until carrel_cad_free.
struct CarrelMesh {
    float* positions;        // x, y, z for each vertex, in millimetres
    uint32_t* indices;       // three for each triangle
    size_t vertex_count;
    size_t triangle_count;
    size_t solids;
    double volume;           // cubic millimetres, 0 when there are no solids
    char* error;             // set when reading failed
};

static char* copy(const char* text) {
    char* out = static_cast<char*>(std::malloc(std::strlen(text) + 1));
    if (out) std::strcpy(out, text);
    return out;
}

static bool read_shape(const char* path, int iges, TopoDS_Shape& shape) {
    if (iges) {
        IGESControl_Reader reader;
        if (reader.ReadFile(path) != IFSelect_RetDone) return false;
        reader.TransferRoots();
        shape = reader.OneShape();
    } else {
        STEPControl_Reader reader;
        if (reader.ReadFile(path) != IFSelect_RetDone) return false;
        reader.TransferRoots();
        shape = reader.OneShape();
    }
    return !shape.IsNull();
}

// Reads the file at `path` (IGES when `iges` is set, STEP otherwise) and meshes it with a
// deflection of `fraction` of its bounding box's diagonal. Returns 0 on success.
int carrel_cad_mesh(const char* path, int iges, double fraction, CarrelMesh* out) {
    std::memset(out, 0, sizeof(*out));
    try {
        // OpenCascade reports to standard output, which is the provider protocol's channel.
        Message::DefaultMessenger()->ChangePrinters().Clear();
        TopoDS_Shape shape;
        if (!read_shape(path, iges, shape)) {
            out->error = copy("the file couldn't be read as a model");
            return 1;
        }
        Bnd_Box box;
        BRepBndLib::Add(shape, box);
        if (box.IsVoid()) {
            out->error = copy("the model has no geometry");
            return 1;
        }
        double diagonal = std::sqrt(box.SquareExtent());
        BRepMesh_IncrementalMesh mesh(shape, diagonal * fraction, Standard_False, 0.5, Standard_True);

        std::vector<float> positions;
        std::vector<uint32_t> indices;
        for (TopExp_Explorer faces(shape, TopAbs_FACE); faces.More(); faces.Next()) {
            const TopoDS_Face& face = TopoDS::Face(faces.Current());
            TopLoc_Location location;
            Handle(Poly_Triangulation) triangulation = BRep_Tool::Triangulation(face, location);
            if (triangulation.IsNull()) continue;
            const gp_Trsf& transform = location.Transformation();
            uint32_t base = static_cast<uint32_t>(positions.size() / 3);
            for (int i = 1; i <= triangulation->NbNodes(); ++i) {
                gp_Pnt point = triangulation->Node(i).Transformed(transform);
                positions.push_back(static_cast<float>(point.X()));
                positions.push_back(static_cast<float>(point.Y()));
                positions.push_back(static_cast<float>(point.Z()));
            }
            bool reversed = face.Orientation() == TopAbs_REVERSED;
            for (int i = 1; i <= triangulation->NbTriangles(); ++i) {
                int a, b, c;
                triangulation->Triangle(i).Get(a, b, c);
                if (reversed) std::swap(b, c);
                indices.push_back(base + a - 1);
                indices.push_back(base + b - 1);
                indices.push_back(base + c - 1);
            }
        }
        for (TopExp_Explorer solids(shape, TopAbs_SOLID); solids.More(); solids.Next()) {
            out->solids += 1;
        }
        if (out->solids > 0) {
            GProp_GProps properties;
            BRepGProp::VolumeProperties(shape, properties);
            out->volume = std::abs(properties.Mass());
        }
        out->vertex_count = positions.size() / 3;
        out->triangle_count = indices.size() / 3;
        out->positions = static_cast<float*>(std::malloc(positions.size() * sizeof(float) + 1));
        out->indices = static_cast<uint32_t*>(std::malloc(indices.size() * sizeof(uint32_t) + 1));
        if (!out->positions || !out->indices) {
            out->error = copy("out of memory");
            return 1;
        }
        std::memcpy(out->positions, positions.data(), positions.size() * sizeof(float));
        std::memcpy(out->indices, indices.data(), indices.size() * sizeof(uint32_t));
        return 0;
    } catch (const Standard_Failure& failure) {
        out->error = copy(failure.GetMessageString() ? failure.GetMessageString() : "OpenCascade failed");
    } catch (const std::exception& failure) {
        out->error = copy(failure.what());
    } catch (...) {
        out->error = copy("OpenCascade failed");
    }
    return 1;
}

void carrel_cad_free(CarrelMesh* mesh) {
    std::free(mesh->positions);
    std::free(mesh->indices);
    std::free(mesh->error);
    std::memset(mesh, 0, sizeof(*mesh));
}

}
