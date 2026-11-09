#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <GL/glx.h>
#include <GL/gl.h>
#include <chrono>
#include <thread>
#include <fstream>
#include <cstring>
#include "imgui.h"
#include "imgui_internal.h"
#include "backends/imgui_impl_opengl3.h"

int main(int argc,char**argv) {
    bool light=false,large=false; for(int i=1;i<argc;i++){light|=strcmp(argv[i],"--light")==0;large|=strcmp(argv[i],"--large")==0;}
    Display* display=XOpenDisplay(nullptr); if(!display)return 1;
    int att[]={GLX_RGBA,GLX_DEPTH_SIZE,24,GLX_DOUBLEBUFFER,None};
    auto visual=glXChooseVisual(display,DefaultScreen(display),att); if(!visual)return 2;
    auto color=XCreateColormap(display,RootWindow(display,visual->screen),visual->visual,AllocNone);
    XSetWindowAttributes attrs{};attrs.colormap=color;attrs.event_mask=ExposureMask|KeyPressMask|KeyReleaseMask|PointerMotionMask|ButtonPressMask|ButtonReleaseMask|StructureNotifyMask;
    Window window=XCreateWindow(display,RootWindow(display,visual->screen),0,0,1280,820,0,visual->depth,InputOutput,visual->visual,CWColormap|CWEventMask,&attrs);
    XStoreName(display,window,"Dear ImGui docking audit"); XMapWindow(display,window);
    auto gl=glXCreateContext(display,visual,nullptr,GL_TRUE);glXMakeCurrent(display,window,gl);
    IMGUI_CHECKVERSION();ImGui::CreateContext();auto&io=ImGui::GetIO();io.ConfigFlags|=ImGuiConfigFlags_DockingEnable;io.IniFilename=nullptr;
    if(light)ImGui::StyleColorsLight();else ImGui::StyleColorsDark();
    io.Fonts->AddFontDefault();if(large)ImGui::GetStyle().FontScaleMain=2.0f;
    ImGui_ImplOpenGL3_Init("#version 130");
    int width=1280,height=820;auto previous=std::chrono::steady_clock::now();
    float value=.42f;bool enabled=true;char text[256]="Select an object to inspect its properties.\nContent follows the application's theme.";
    const char*titles[]={"Scene","Document.rs","History","Profiler","Animation","Material settings and texture references","Assets","Console","Inspector","Preview"};
    for(int frame=0;frame<3600;frame++) {
        while(XPending(display)){XEvent ev;XNextEvent(display,&ev);if(ev.type==ConfigureNotify){width=ev.xconfigure.width;height=ev.xconfigure.height;}if(ev.type==MotionNotify)io.AddMousePosEvent((float)ev.xmotion.x,(float)ev.xmotion.y);if(ev.type==ButtonPress||ev.type==ButtonRelease){bool down=ev.type==ButtonPress;int b=ev.xbutton.button;if(b<=3)io.AddMouseButtonEvent(b==1?0:b==3?1:2,down);if(down&&(b==4||b==5))io.AddMouseWheelEvent(0,b==4?1:-1);}if(ev.type==KeyPress&&XLookupKeysym(&ev.xkey,0)==XK_Escape)io.AddKeyEvent(ImGuiKey_Escape,true);if(ev.type==KeyRelease&&XLookupKeysym(&ev.xkey,0)==XK_Escape)io.AddKeyEvent(ImGuiKey_Escape,false);}
        auto now=std::chrono::steady_clock::now();io.DeltaTime=std::chrono::duration<float>(now-previous).count();previous=now;io.DisplaySize=ImVec2((float)width,(float)height);
        ImGui_ImplOpenGL3_NewFrame();ImGui::NewFrame();
        ImGui::SetNextWindowPos(ImVec2(0,0));ImGui::SetNextWindowSize(ImVec2((float)width-280,(float)height));
        ImGui::PushStyleVar(ImGuiStyleVar_WindowRounding,0);ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize,0);ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding,ImVec2(0,0));
        ImGui::Begin("Dock host",nullptr,ImGuiWindowFlags_NoTitleBar|ImGuiWindowFlags_NoResize|ImGuiWindowFlags_NoMove|ImGuiWindowFlags_NoCollapse|ImGuiWindowFlags_NoDocking);
        ImGui::PopStyleVar(3);auto id=ImGui::GetID("audit-dockspace");
        if(frame==0){ImGui::DockBuilderRemoveNode(id);ImGui::DockBuilderAddNode(id,ImGuiDockNodeFlags_DockSpace);ImGui::DockBuilderSetNodeSize(id,ImVec2((float)width-280,(float)height));ImGuiID center=id;ImGuiID left=ImGui::DockBuilderSplitNode(center,ImGuiDir_Left,.24f,nullptr,&center);ImGui::DockBuilderDockWindow("Scene",left);ImGui::DockBuilderDockWindow("Assets",left);for(int j=1;j<=5;j++)ImGui::DockBuilderDockWindow(titles[j],center);ImGui::DockBuilderFinish(id);
            ImGuiID floating=ImGui::GetID("floating-audit");ImGui::DockBuilderAddNode(floating);ImGui::DockBuilderSetNodePos(floating,ImVec2(440,265));ImGui::DockBuilderSetNodeSize(floating,ImVec2(380,310));ImGui::DockBuilderDockWindow("Inspector",floating);ImGui::DockBuilderDockWindow("Preview",floating);ImGui::DockBuilderFinish(floating);}
        ImGui::DockSpace(id);ImGui::End();
        for(int j=0;j<10;j++){if(j==7)continue;bool open=true;ImGui::Begin(titles[j],&open);ImGui::TextUnformatted("Application-owned content");if(ImGui::Button("Apply")){}ImGui::SameLine();if(ImGui::Button("Reset")){}ImGui::Checkbox("Enabled",&enabled);ImGui::SliderFloat("Value",&value,0,1);ImGui::InputTextMultiline("##content",text,sizeof text,ImVec2(-1,110));ImGui::End();}
        ImGui::SetNextWindowPos(ImVec2((float)width-270,35),ImGuiCond_Always);ImGui::SetNextWindowSize(ImVec2(260,350),ImGuiCond_Always);
        ImGui::Begin("Native ImGui Window",nullptr,ImGuiWindowFlags_NoDocking);ImGui::TextUnformatted("Default docking branch theme");if(ImGui::Button("Button")){}ImGui::SameLine();ImGui::Selectable("Selected",true);ImGui::Checkbox("Enabled",&enabled);ImGui::SliderFloat("Value",&value,0,1);ImGui::InputTextMultiline("##native",text,sizeof text,ImVec2(-1,110));ImGui::End();
        ImGui::Render();glViewport(0,0,width,height);glClearColor(.15f,.15f,.15f,1);glClear(GL_COLOR_BUFFER_BIT);ImGui_ImplOpenGL3_RenderDrawData(ImGui::GetDrawData());glXSwapBuffers(display,window);
        std::this_thread::sleep_for(std::chrono::milliseconds(16));
    }
}
