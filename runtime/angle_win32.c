/* SwitchRecomp by MZ Labs; fork additions, Apache-2.0.
   ANGLE owns GLES -> Vulkan / Direct3D 11 translation. Never fall back to WGL
   entry points after selecting an EGL context. */
#include "angle_win32.h"
#include <stdio.h>
#include <string.h>
#include <wchar.h>
typedef void* ED; typedef void* EC; typedef void* ES; typedef void* EF;
typedef int EI; typedef unsigned int EB;
static HMODULE egl, gles;
static ED display; static EC context, worker; static ES surface, pbuffer;
static char error_text[512];
static void* (WINAPI* get_proc)(const char*);
static EB (WINAPI* make_current)(ED,ES,ES,EC);
static EB (WINAPI* swap_buffers)(ED,ES);
static EI (WINAPI* get_error)(void);
static int fail(const char* message){
  snprintf(error_text,sizeof error_text,"%s (EGL 0x%x)",message,get_error? get_error() : 0);
  return 0;
}
const char* swr_angle_error(void){ return error_text; }
int swr_angle_active(void){ return context!=NULL; }
void* swr_angle_proc(const char* n){
  void* p=get_proc? get_proc(n) : NULL;
  return p? p : (void*)GetProcAddress(gles,n);
}
int swr_angle_swap(void){
  if(swap_buffers && swap_buffers(display,surface)) return 1;
  return fail("eglSwapBuffers failed");
}
int swr_angle_worker_current(void){ return worker && make_current(display,pbuffer,pbuffer,worker); }

int swr_angle_create(HWND window, const char* renderer, int vsync){
  wchar_t dir[32768], path[32768];
  DWORD n=GetEnvironmentVariableW(L"SWR_ANGLE_DIR",dir,32768);
  if(!n){
    n=GetModuleFileNameW(NULL,dir,32768);
    if(!n || n>=32700) return fail("Cannot locate ANGLE libraries");
    wchar_t* slash=wcsrchr(dir,L'\\'); if(!slash) return fail("Cannot locate application folder");
    slash[1]=0; wcscat(dir,L"angle");
  } else if(n>=32700) return fail("ANGLE library path is too long");
  if(wcslen(dir)>32680) return fail("ANGLE library path is too long");
  /* Preload the pinned Vulkan loader by absolute path. ANGLE's later lookup by
     module name must not find an unrelated library in the current directory. */
  swprintf(path,32768,L"%ls\\vulkan-1.dll",dir);
  LoadLibraryExW(path,NULL,LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|LOAD_LIBRARY_SEARCH_SYSTEM32);
  swprintf(path,32768,L"%ls\\vk_swiftshader.dll",dir);
  if(!strcmp(renderer,"vulkan-software"))
    LoadLibraryExW(path,NULL,LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|LOAD_LIBRARY_SEARCH_SYSTEM32);
  swprintf(path,32768,L"%ls\\libGLESv2.dll",dir);
  gles=LoadLibraryExW(path,NULL,LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|LOAD_LIBRARY_SEARCH_SYSTEM32);
  swprintf(path,32768,L"%ls\\libEGL.dll",dir);
  egl=LoadLibraryExW(path,NULL,LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|LOAD_LIBRARY_SEARCH_SYSTEM32);
  if(!egl || !gles) return fail("Bundled ANGLE DLLs are missing; recompile with the updated app");
#define EGLFN(type,name,args) type (WINAPI* name) args=(void*)GetProcAddress(egl,#name); if(!name) return fail("Missing " #name)
  EGLFN(EB,eglInitialize,(ED,EI*,EI*));
  EGLFN(EB,eglBindAPI,(unsigned));
  EGLFN(EB,eglChooseConfig,(ED,const EI*,EF*,EI,EI*));
  EGLFN(EC,eglCreateContext,(ED,EF,EC,const EI*));
  EGLFN(ES,eglCreateWindowSurface,(ED,EF,HWND,const EI*));
  EGLFN(ES,eglCreatePbufferSurface,(ED,EF,const EI*));
  EGLFN(EB,eglSwapInterval,(ED,EI));
#undef EGLFN
  get_proc=(void*)GetProcAddress(egl,"eglGetProcAddress");
  get_error=(void*)GetProcAddress(egl,"eglGetError");
  make_current=(void*)GetProcAddress(egl,"eglMakeCurrent");
  swap_buffers=(void*)GetProcAddress(egl,"eglSwapBuffers");
  if(!get_proc || !make_current || !swap_buffers) return fail("Incomplete EGL library");
  ED (WINAPI* platform_display)(unsigned,void*,const EI*)=(void*)get_proc("eglGetPlatformDisplayEXT");
  if(!platform_display) return fail("EGL_ANGLE_platform_angle is unavailable");
  int vk=!strcmp(renderer,"vulkan") || !strcmp(renderer,"vulkan-software");
  int software=!strcmp(renderer,"vulkan-software") || !strcmp(renderer,"d3d11-warp");
  if(!vk && strcmp(renderer,"d3d11") && strcmp(renderer,"d3d11-warp")) return fail("Unknown ANGLE renderer");
  EI da[]={0x3203,vk? 0x3450 : 0x3208,0x3209,software? (vk? 0x3487 : 0x320B) : 0x320A,0x3038};
  display=platform_display(0x3202,NULL,da);
  if(!display || !eglInitialize(display,NULL,NULL)) return fail("Selected Vulkan/DirectX device is unavailable");
  if(!eglBindAPI(0x30A0)) return fail("Cannot select GLES API");
  EI ca[]={0x3040,vk? 0x40 : 4,0x3033,window? 5 : 1,0x3024,8,0x3023,8,0x3022,8,0x3021,8,0x3025,24,0x3026,8,0x3038};
  EF config=NULL; EI count=0;
  if(!eglChooseConfig(display,ca,&config,1,&count) || !count) return fail("No compatible EGL framebuffer configuration");
  EI at[]={0x3098,3,0x30FB,vk? (!strcmp(renderer,"vulkan-software")? 1 : 2) : 0,0x3038};
  context=eglCreateContext(display,config,NULL,at);
  if(!context) return fail(vk? "Vulkan backend requires GLES 3.2" : "DirectX backend requires GLES 3.0");
  EI pa[]={0x3057,32,0x3056,32,0x3038};
  pbuffer=eglCreatePbufferSurface(display,config,pa);
  surface=window? eglCreateWindowSurface(display,config,window,NULL) : pbuffer;
  if(!surface || !make_current(display,surface,surface,context)){
    context=NULL; return fail("Cannot make the EGL context current");
  }
  eglSwapInterval(display,vsync? 1 : 0);
  worker=eglCreateContext(display,config,context,at);
  if(!pbuffer) worker=NULL;
  return 1;
}
