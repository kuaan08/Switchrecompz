/* Synthetic graphics integration test. No game shaders or assets. Apache-2.0. */
#include "glfn.h"
#include "shader.h"
#include "angle_win32.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
int swr_gl_load(void* (*gp)(const char*));
extern int g_glsl_es, g_glsl_es_version, g_shader_ubo_stride, g_rs_binding;
int g_texoff_emul;
static GLuint compile_probe(GLenum stage,const char* source){
  GLuint s=glCreateShader(stage); glShaderSource(s,1,&source,NULL); glCompileShader(s);
  GLint ok=0; glGetShaderiv(s,GL_COMPILE_STATUS,&ok);
  if(!ok){ char error[4096]; glGetShaderInfoLog(s,sizeof error,NULL,error); fprintf(stderr,"%s\n%s\n",error,source); exit(2); }
  return s;
}
int main(int argc,char** argv){
  if(argc!=2) return 2;
  if(!swr_angle_create(NULL,argv[1],0)){ fprintf(stderr,"%s\n",swr_angle_error()); return 3; }
  if(!swr_gl_load(swr_angle_proc)){ fprintf(stderr,"Missing GLES runtime entry points\n"); return 4; }
  printf("%s: %s / %s\n",argv[1],glGetString(GL_RENDERER),glGetString(GL_VERSION));
  GLint major=3,minor=0;
  glGetIntegerv(GL_MAJOR_VERSION,&major); glGetIntegerv(GL_MINOR_VERSION,&minor);
  g_glsl_es=1; g_glsl_es_version=major*100+minor*10;
  if(g_glsl_es_version<300 || g_glsl_es_version>320){
    fprintf(stderr,"Unsupported GLES version %d\n",g_glsl_es_version); return 10;
  }
  int legacy=g_glsl_es_version==300;
  GLint max_ubo=24; glGetIntegerv(GL_MAX_UNIFORM_BUFFER_BINDINGS,&max_ubo);
  g_rs_binding=max_ubo-1;
  g_shader_ubo_stride=(max_ubo-1)/(legacy?2:3);
  if(g_shader_ubo_stride>18) g_shader_ubo_stride=18;
  if(g_shader_ubo_stride<4) return 11;
  printf("capabilities: GLES %d, UBO bindings=%d, stage stride=%d, compute=%s\n",
         g_glsl_es_version,max_ubo,g_shader_ubo_stride,legacy?"no":"yes");
  u64 code[14]={0};
  code[11]=0xEFD87F800807FF00ull | (3ull<<47);
  code[12]=0xEFF07F800707FF00ull | (2ull<<47);
  code[13]=0xE30000000007000Full;
  ShaderInfo info={0}; char* vs=shader_to_glsl((u8*)code,sizeof code,ST_VS,&info);
  if(!vs || info.unsupported) return 5;
  char fs[1024];
  snprintf(fs,sizeof fs,legacy?
    "#version 300 es\nprecision highp float; layout(std140) uniform S4_CB3{vec4 tint;};uniform sampler2D S4_tex0;out vec4 color;void main(){color=texture(S4_tex0,vec2(.5))*tint;}" :
    "#version %d es\nprecision highp float; layout(std140,binding=%d) uniform S4_CB3{vec4 tint;};layout(binding=16) uniform sampler2D S4_tex0;out vec4 color;void main(){color=texture(S4_tex0,vec2(.5))*tint;}",
    g_glsl_es_version,g_shader_ubo_stride+3);
  GLuint program=glCreateProgram(); glAttachShader(program,compile_probe(GL_VERTEX_SHADER,vs)); glAttachShader(program,compile_probe(GL_FRAGMENT_SHADER,fs)); free(vs);
  glLinkProgram(program); GLint ok=0; glGetProgramiv(program,GL_LINK_STATUS,&ok);
  if(!ok){ char error[4096]; glGetProgramInfoLog(program,sizeof error,NULL,error); fprintf(stderr,"%s\n",error); return 6; }
  GLuint vao,vbo,ubo,tex;
  glGenVertexArrays(1,&vao); glBindVertexArray(vao);
  float vertices[]={-.8f,-.8f,0,9,.8f,-.8f,0,9,0,.8f,0,9};
  glGenBuffers(1,&vbo); glBindBuffer(GL_ARRAY_BUFFER,vbo); glBufferData(GL_ARRAY_BUFFER,sizeof vertices,vertices,GL_STATIC_DRAW);
  float copy[12]={0}; glGetBufferSubData(GL_ARRAY_BUFFER,0,sizeof copy,copy);
  if(memcmp(copy,vertices,sizeof copy)){ fprintf(stderr,"Buffer readback mismatch\n"); return 7; }
  glEnableVertexAttribArray(0); glVertexAttribPointer(0,4,GL_FLOAT,GL_FALSE,0,0);
  float tint[]={1,.5f,.25f,1};
  glGenBuffers(1,&ubo); glBindBuffer(GL_UNIFORM_BUFFER,ubo); glBufferData(GL_UNIFORM_BUFFER,sizeof tint,tint,GL_STATIC_DRAW);
  glBindBufferBase(GL_UNIFORM_BUFFER,g_shader_ubo_stride+3,ubo);
  unsigned char white[]={255,255,255,255};
  glActiveTexture(GL_TEXTURE0+16); glGenTextures(1,&tex); glBindTexture(GL_TEXTURE_2D,tex);
  glTexStorage2D(GL_TEXTURE_2D,1,GL_RGBA8,1,1); glTexSubImage2D(GL_TEXTURE_2D,0,0,0,1,1,GL_RGBA,GL_UNSIGNED_BYTE,white);
  glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_NEAREST); glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_NEAREST);
  glUseProgram(program); glViewport(0,0,32,32);
  if(legacy) glColorMaski(0,1,1,1,1); else glColorMask(1,1,1,1);
  glClearColor(0,0,0,1); glClear(GL_COLOR_BUFFER_BIT); glDrawArrays(GL_TRIANGLES,0,3);
  glFinish(); unsigned char pixel[4]={0}; glReadPixels(16,16,1,1,GL_RGBA,GL_UNSIGNED_BYTE,pixel);
  GLenum error=glGetError();
  printf("pixel=%u,%u,%u,%u error=%x\n",pixel[0],pixel[1],pixel[2],pixel[3],error);
  if(error || pixel[0]!=255 || abs((int)pixel[1]-128)>1 || abs((int)pixel[2]-64)>1 || pixel[3]!=255) return 8;
  if(!swr_angle_swap()) return 9;
  /* GLES 3.1 is a real capability distinction, not just a more modern
     version string. Verify compute, SSBO writes and a memory barrier. */
  if(g_glsl_es_version>=310){
    const char* code="#version 310 es\nlayout(local_size_x=1) in; layout(std430,binding=0) buffer Output{uint n;}; void main(){n=42u;}";
    GLuint cs=compile_probe(GL_COMPUTE_SHADER,code);
    GLuint prog=glCreateProgram(); glAttachShader(prog,cs); glLinkProgram(prog);
    GLint linked=0; glGetProgramiv(prog,GL_LINK_STATUS,&linked);
    if(!linked){ fprintf(stderr,"GLES 3.1 compute program did not link\n"); return 12; }
    GLuint storage=0; glGenBuffers(1,&storage); glBindBuffer(GL_SHADER_STORAGE_BUFFER,storage);
    GLuint zero=0; glBufferData(GL_SHADER_STORAGE_BUFFER,sizeof zero,&zero,GL_DYNAMIC_DRAW);
    glBindBufferBase(GL_SHADER_STORAGE_BUFFER,0,storage);
    glUseProgram(prog); glDispatchCompute(1,1,1);
    glMemoryBarrier(GL_SHADER_STORAGE_BARRIER_BIT | GL_BUFFER_UPDATE_BARRIER_BIT);
    glFinish(); GLuint result=0;
    glGetBufferSubData(GL_SHADER_STORAGE_BUFFER,0,sizeof result,&result);
    GLenum err=glGetError();
    if(err || result!=42u){ fprintf(stderr,"GLES 3.1 compute failed: value=%u error=%x\n",result,err); return 13; }
    glDeleteBuffers(1,&storage); glDeleteProgram(prog); glDeleteShader(cs);
    puts("PASS: GLES 3.1 compute shader and SSBO readback");
  }
  puts("PASS: translated XYZ shader, texture, UBO, draw, readback and presentation"); return 0;
}
