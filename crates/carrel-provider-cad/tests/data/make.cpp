// Writes tests/data/bracket.step and bracket.igs with OpenCascade: an L-bracket and a pin,
// two solids. Build with clang++ -std=c++17 against OpenCascade, as build.rs finds it.

#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRep_Builder.hxx>
#include <IGESControl_Controller.hxx>
#include <IGESControl_Writer.hxx>
#include <STEPControl_Writer.hxx>
#include <TopoDS_Compound.hxx>
#include <IGESData_GlobalSection.hxx>
#include <IGESData_IGESModel.hxx>
#include <TCollection_HAsciiString.hxx>
#include <gp_Ax2.hxx>
int main(int, char** argv) {
    // An L-bracket 40 x 20 x 30 mm and, beside it, a pin 6 mm across and 25 mm long.
    TopoDS_Shape base = BRepPrimAPI_MakeBox(40, 20, 3).Shape();
    TopoDS_Shape upright = BRepPrimAPI_MakeBox(40, 3, 30).Shape();
    TopoDS_Shape bracket = BRepAlgoAPI_Fuse(base, upright).Shape();
    TopoDS_Shape pin = BRepPrimAPI_MakeCylinder(gp_Ax2(gp_Pnt(60, 10, 0), gp_Dir(0, 0, 1)), 3, 25).Shape();
    TopoDS_Compound both; BRep_Builder builder; builder.MakeCompound(both);
    builder.Add(both, bracket); builder.Add(both, pin);
    STEPControl_Writer step; step.Transfer(both, STEPControl_AsIs); step.Write(argv[1]);
    IGESControl_Controller::Init();
    IGESControl_Writer iges("MM", 1); iges.AddShape(both); iges.ComputeModel();
    // Not the account that ran it, which the writer would otherwise record.
    IGESData_GlobalSection global = iges.Model()->GlobalSection();
    global.SetAuthorName(new TCollection_HAsciiString("Carrel tests"));
    iges.Model()->SetGlobalSection(global);
    iges.Write(argv[2]);
}
