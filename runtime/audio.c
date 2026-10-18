/* SwitchRecomp - Copyright 2026 MZ Labs
   Licensed under the Apache License 2.0. See LICENSE.txt and NOTICE.txt;
   any copy or derived work must keep the credit to SwitchRecomp by MZ Labs. */
/* Audio: the audio renderer (audren:u), audout:u and the sound card.
   On the console the DSP renders 5 ms at a time from the voices, mixes and sinks a game describes with
   RequestUpdate. A thread here does the same: it decodes the wave buffers (PCM8/16/24/32/float and
   Nintendo ADPCM), resamples them to the renderer rate, applies the voice biquads and volumes, routes them
   through the submixes and splitters to the final mix and the device sink, and sends the stereo result to
   the sound card: waveOut on Windows, SDL2, PulseAudio/PipeWire or ALSA on Linux. The sound card's clock
   paces the thread, and every period signals the event games wait on before their next update. The
   buffer mixer, biquad filter, light limiter and reverb effects are applied (the reverbs as a small
   Schroeder reverb); aux, delay, capture and compressor pass the sound through.
   Parameter layouts as in yuzu's audio_core (InfoUpdater and friends). */
#include "rt.h"
#include <math.h>

#define OUT_RATE 48000
#define PERIOD   240              /* frames per 5 ms at 48 kHz */
#define LATENCY  8                /* periods queued in the sound card at most (40 ms) */

int g_opt_volume=100;             /* volume= in the .ini, 0 to 100 */
static KEvent* g_ren_ev;          /* renderer event, signalled every period */
static PMutex g_alk;              /* the renderer and audout lists */
static volatile int g_want_host;  /* a game opened audio: open the sound card */

/* ============================================================ sound card */
#if defined(_WIN32)
#include <windows.h>
#include <mmsystem.h>
#define NHDR 16
static HWAVEOUT g_wo; static WAVEHDR g_hdr[NHDR]; static i16 g_wpcm[NHDR][PERIOD*2]; static HANDLE g_wev; static int g_hnext;
static int host_open(void){
  WAVEFORMATEX wf; memset(&wf,0,sizeof wf); wf.wFormatTag=WAVE_FORMAT_PCM; wf.nChannels=2; wf.nSamplesPerSec=OUT_RATE;
  wf.wBitsPerSample=16; wf.nBlockAlign=4; wf.nAvgBytesPerSec=OUT_RATE*4;
  g_wev=CreateEventA(NULL,FALSE,FALSE,NULL);
  MMRESULT e=waveOutOpen(&g_wo,WAVE_MAPPER,&wf,(DWORD_PTR)g_wev,0,CALLBACK_EVENT);
  if(e!=MMSYSERR_NOERROR){ WARN("audio: no sound device (waveOut error %u), the game runs muted\n",(unsigned)e); return 0; }
  for(int i=0;i<NHDR;i++){ g_hdr[i].lpData=(LPSTR)g_wpcm[i]; g_hdr[i].dwBufferLength=sizeof g_wpcm[i]; waveOutPrepareHeader(g_wo,&g_hdr[i],sizeof(WAVEHDR)); }
  LOG("audio: sound device open (waveOut, 48 kHz stereo)\n"); return 1; }
/* periods the device can take now: the driver clears WHDR_INQUEUE as it finishes each buffer */
static int host_room(void){
  int busy=0; for(int i=0;i<NHDR;i++) if(((volatile WAVEHDR*)&g_hdr[i])->dwFlags&WHDR_INQUEUE) busy++;
  if(((volatile WAVEHDR*)&g_hdr[g_hnext])->dwFlags&WHDR_INQUEUE) return 0;
  return busy<LATENCY? LATENCY-busy : 0; }
static void host_write(const i16* pcm){
  memcpy(g_wpcm[g_hnext],pcm,sizeof g_wpcm[0]);
  MMRESULT result=waveOutWrite(g_wo,&g_hdr[g_hnext],sizeof(WAVEHDR));
  if(result!=MMSYSERR_NOERROR){
    static int errors; if(errors++<8) WARN("audio: waveOutWrite failed with error %u\n",(unsigned)result);
  }
  g_hnext=(g_hnext+1)%NHDR; }
static void host_wait(void){ WaitForSingleObject(g_wev,5); }
#elif defined(__linux__) && !defined(__ANDROID__)
#include <dlfcn.h>
/* SDL2 when it's installed, else PulseAudio (or PipeWire's server for it), else ALSA; all loaded at run time */
#define SYM(h,f,n) (*(void**)&(f)=dlsym((h),(n)))
typedef struct { int freq; u16 format; u8 channels, silence; u16 samples, padding; u32 size; void* callback; void* userdata; } SdlSpec;
static u32 g_sdev; static int (*s_queue)(u32,const void*,u32); static u32 (*s_queued)(u32);
static void* g_apcm; static long (*a_writei)(void*,const void*,unsigned long); static int (*a_recover)(void*,int,int);
static int host_open_sdl(void){
  void* h=dlopen("libSDL2-2.0.so.0",RTLD_NOW|RTLD_LOCAL); if(!h) return 0;
  int (*init)(u32); u32 (*opn)(const char*,int,const SdlSpec*,SdlSpec*,int); void (*pause)(u32,int); const char* (*err)(void);
  if(!SYM(h,init,"SDL_InitSubSystem") || !SYM(h,opn,"SDL_OpenAudioDevice") || !SYM(h,pause,"SDL_PauseAudioDevice") ||
     !SYM(h,s_queue,"SDL_QueueAudio") || !SYM(h,s_queued,"SDL_GetQueuedAudioSize") || !SYM(h,err,"SDL_GetError")) return 0;
  if(init(0x10 /* SDL_INIT_AUDIO */)){ WARN("audio: SDL2 has no audio (%s)\n",err()); return 0; }
  SdlSpec want; memset(&want,0,sizeof want); want.freq=OUT_RATE; want.format=0x8010 /* AUDIO_S16LSB */; want.channels=2; want.samples=512;
  g_sdev=opn(NULL,0,&want,NULL,0);
  if(!g_sdev){ WARN("audio: SDL2 could not open the sound device (%s)\n",err()); return 0; }
  pause(g_sdev,0);
  LOG("audio: sound device open (SDL2, 48 kHz stereo)\n"); return 1; }
/* PulseAudio's simple API, also served by PipeWire: for desktops without SDL2 */
static void* g_pa; static int (*p_write)(void*,const void*,size_t,int*);
static int host_open_pulse(void){
  void* h=dlopen("libpulse-simple.so.0",RTLD_NOW|RTLD_LOCAL); if(!h) return 0;
  typedef struct { int format; u32 rate; u8 channels; } PaSpec;
  typedef struct { u32 maxlength, tlength, prebuf, minreq, fragsize; } PaAttr;
  void* (*nw)(const char*,const char*,int,const char*,const char*,const PaSpec*,const void*,const PaAttr*,int*);
  if(!SYM(h,nw,"pa_simple_new") || !SYM(h,p_write,"pa_simple_write")) return 0;
  PaSpec ss={3 /* S16LE */,OUT_RATE,2}; PaAttr at={0xFFFFFFFFu,LATENCY*PERIOD*4,0xFFFFFFFFu,0xFFFFFFFFu,0xFFFFFFFFu}; int err=0;
  g_pa=nw(NULL,"SwitchRecomp",1 /* playback */,NULL,g_exename[0]? g_exename : "game",&ss,NULL,&at,&err);
  if(!g_pa){ WARN("audio: PulseAudio could not open a stream (error %d)\n",err); return 0; }
  LOG("audio: sound device open (PulseAudio, 48 kHz stereo)\n"); return 1; }
static int host_open_alsa(void){
  void* h=dlopen("libasound.so.2",RTLD_NOW|RTLD_LOCAL); if(!h) return 0;
  int (*op)(void**,const char*,int,int); int (*sp)(void*,int,int,unsigned,unsigned,int,unsigned);
  if(!SYM(h,op,"snd_pcm_open") || !SYM(h,sp,"snd_pcm_set_params") || !SYM(h,a_writei,"snd_pcm_writei") || !SYM(h,a_recover,"snd_pcm_recover")) return 0;
  int e=op(&g_apcm,"default",0 /* playback */,0);
  if(e<0){ g_apcm=NULL; WARN("audio: ALSA could not open the default device (error %d)\n",e); return 0; }
  if(sp(g_apcm,2 /* S16_LE */,3 /* RW_INTERLEAVED */,2,OUT_RATE,1,LATENCY*5000)<0){ g_apcm=NULL; return 0; }
  LOG("audio: sound device open (ALSA, 48 kHz stereo)\n"); return 1; }
static int host_open(void){
  if(host_open_sdl() || host_open_pulse() || host_open_alsa()) return 1;
  WARN("audio: no sound device (SDL2, PulseAudio/PipeWire or ALSA), the game runs muted\n"); return 0; }
static int host_room(void){
  if(!g_sdev) return 1;   /* PulseAudio and ALSA: the write blocks until there is room */
  u32 q=s_queued(g_sdev), cap=LATENCY*PERIOD*4; return q<cap? (int)((cap-q)/(PERIOD*4)) : 0; }
static void host_write(const i16* pcm){
  if(g_sdev){ s_queue(g_sdev,pcm,PERIOD*4); return; }
  if(g_pa){ int err; p_write(g_pa,pcm,PERIOD*4,&err); return; }
  long n=a_writei(g_apcm,pcm,PERIOD); if(n<0 && a_recover(g_apcm,(int)n,1)>=0) a_writei(g_apcm,pcm,PERIOD); }
static void host_wait(void){ plat_sleep_ns(1000000); }
#elif defined(__ANDROID__)
#include <aaudio/AAudio.h>
static AAudioStream* g_aa;
static int host_open(void){
  AAudioStreamBuilder* b=NULL;
  aaudio_result_t r=AAudio_createStreamBuilder(&b);
  if(r!=AAUDIO_OK) return 0;
  AAudioStreamBuilder_setDirection(b,AAUDIO_DIRECTION_OUTPUT);
  AAudioStreamBuilder_setFormat(b,AAUDIO_FORMAT_PCM_I16);
  AAudioStreamBuilder_setChannelCount(b,2);
  AAudioStreamBuilder_setSampleRate(b,OUT_RATE);
  AAudioStreamBuilder_setSharingMode(b,AAUDIO_SHARING_MODE_SHARED);
  AAudioStreamBuilder_setPerformanceMode(b,AAUDIO_PERFORMANCE_MODE_LOW_LATENCY);
  r=AAudioStreamBuilder_openStream(b,&g_aa); AAudioStreamBuilder_delete(b);
  if(r!=AAUDIO_OK){ WARN("audio: AAudio open failed (%d)\n",r); return 0; }
  if(AAudioStream_getSampleRate(g_aa)!=OUT_RATE || AAudioStream_getChannelCount(g_aa)!=2 ||
     AAudioStream_getFormat(g_aa)!=AAUDIO_FORMAT_PCM_I16 || AAudioStream_requestStart(g_aa)!=AAUDIO_OK){
    AAudioStream_close(g_aa); g_aa=NULL; WARN("audio: AAudio format/start failed\n"); return 0; }
  LOG("audio: AAudio, 48 kHz stereo\n"); return 1;
}
static int host_room(void){ return g_aa? 1 : 0; }
static void host_write(const i16* pcm){
  int sent=0;
  while(g_aa && sent<PERIOD){
    aaudio_result_t n=AAudioStream_write(g_aa,pcm+sent*2,PERIOD-sent,20000000);
    if(n<0){ WARN("audio: AAudio disconnected (%d), reopening\n",n);
      AAudioStream_close(g_aa); g_aa=NULL; host_open(); break; }
    if(!n) break;
    sent+=n;
  }
}
static void host_wait(void){ if(!g_aa) plat_sleep_ns(5000000); }
#else
static int host_open(void){ return 0; }
static int host_room(void){ return 0; }
static void host_write(const i16* pcm){ (void)pcm; }
static void host_wait(void){ }
#endif

/* SWR_AUDIO_WAV=path records everything the game plays, for bug reports and tests */
static FILE* g_wav; static u64 g_wav_frames;
static void wav_header(void){
  u32 data=(u32)(g_wav_frames*4); u8 h[44]; memcpy(h,"RIFF",4); u32 v=36+data; memcpy(h+4,&v,4); memcpy(h+8,"WAVEfmt ",8);
  v=16; memcpy(h+16,&v,4); u16 s=1; memcpy(h+20,&s,2); s=2; memcpy(h+22,&s,2); v=OUT_RATE; memcpy(h+24,&v,4); v=OUT_RATE*4; memcpy(h+28,&v,4);
  s=4; memcpy(h+32,&s,2); s=16; memcpy(h+34,&s,2); memcpy(h+36,"data",4); memcpy(h+40,&data,4);
  fseek(g_wav,0,SEEK_SET); fwrite(h,1,44,g_wav); fseek(g_wav,0,SEEK_END); }
static void wav_write(const i16* pcm){
  static int tried; if(!tried){ tried=1; const char* p=getenv("SWR_AUDIO_WAV"); if(p && *p){ g_wav=fopen(p,"wb"); if(g_wav){ wav_header(); LOG("audio: recording to %s\n",p); } } }
  if(!g_wav) return;
  fwrite(pcm,4,PERIOD,g_wav); g_wav_frames+=PERIOD;
  if(g_wav_frames%(OUT_RATE/2)==0){ wav_header(); fflush(g_wav); } }

/* ============================================================ audio renderer */
enum { FMT_PCM8=1, FMT_PCM16=2, FMT_PCM24=3, FMT_PCM32=4, FMT_FLOAT=5, FMT_ADPCM=6 };
#define UNUSED_MIX 0x7FFFFFFF
typedef struct { u64 addr, size, ctx; i32 start, end, loop_start, loop_end, loop_count; u8 pending, valid, loop; } WBuf;
typedef struct {
  u8 in_use, fmt, state, nch;            /* state: 0 started, 1 stopped, 2 paused */
  u32 rate, mix, res[6]; i32 split;
  float pitch, vol, vol0;                /* vol0: where the last period ended, ramped from */
  struct { u8 on; float b0, b1, b2, a1, a2; } bq[2];
  i16 coef[16];
  WBuf wb[4];
  int cur, loops; i32 off; double frac;
  float s0[6], s1[6], bqs[6][2][2];
  i32 yn1, yn2;
  u64 played; u32 consumed;
} Voice;
typedef struct { u8 in_use; float vol[24], vol0[24]; } ChanRes;
typedef struct { u8 in_use; float volume; i32 nbuf, base, dst, dsplit, depth; float mv[24][24]; } Mix;
typedef struct { u8 in_use; u32 mix; float vol[24]; } SplitDst;
typedef struct { i32 n; i32 dst[48]; } Split;
/* reverb: 4 damped combs and 2 allpasses per channel (Schroeder/Freeverb), lengths for 48 kHz */
static const int VERB_COMB[4]={1695,1760,1623,1548}, VERB_AP[2]={605,480};
typedef struct { float* line[6][6]; int pos[6][6]; float lp[6][4]; } Verb;
typedef struct {
  u8 type, on; u32 mix, order; u8 sp[0xA0];
  float bq[6][2];                                  /* biquad filter */
  float env[6], att[6], la[6][64]; int lapos[6];   /* light limiter */
  Verb* verb;
} Effect;
typedef struct { u8 type, in_use, downmix; u32 nin; i8 in[6]; float coef[4]; u64 addr; u32 size, pos; } Sink;
typedef struct AudRen {
  PMutex lk; int started, rev; u32 rate, count, limit;
  u32 nvoice, nmix, nmixbuf, nsink, neff, nsplit, nsdst;
  Voice* v; ChanRes* cr; Mix* mix; Sink* sk; Effect* ef; Split* sp; SplitDst* sd; float* buf; i32* order; i32 norder;
  float vo[6][PERIOD];                   /* one voice, per channel */
  float o48[2][PERIOD]; float rs_prev[2];
  u64 frames;
  struct AudRen* next;
} AudRen;
static AudRen* g_rens;

static int mapped(u64 a, u64 n){
  if(!a || !n || a+n<a) return 0;
  MemBlock b; mem_query(a,&b); return b.state!=MS_FREE && b.state!=MS_INACCESSIBLE && a+n<=b.addr+b.size; }
static u32 rd32(const u8* p){ u32 v; memcpy(&v,p,4); return v; }
static float rdf(const u8* p){ float v; memcpy(&v,p,4); return v; }

/* bytes a wave buffer needs up to sample frame `end` */
static u64 wb_bytes(int fmt, int nch, i64 end){
  switch(fmt){ case FMT_PCM8: return (u64)end*nch; case FMT_PCM16: return (u64)end*nch*2; case FMT_PCM24: return (u64)end*nch*3;
    case FMT_PCM32: case FMT_FLOAT: return (u64)end*nch*4; case FMT_ADPCM: return (u64)((end+13)/14)*8; }
  return 0; }
static void adpcm_ctx(Voice* v, u64 ctx){ if(ctx && mapped(ctx,6)){ i16 h[3]; memcpy(h,GP(ctx),6); v->yn1=h[1]; v->yn2=h[2]; } }

/* the next sample frame of a voice (all its channels), 0 when it has no data */
static int next_frame(AudRen* r, Voice* v, float* f){
  for(int guard=0;guard<8;guard++){
    WBuf* w=&v->wb[v->cur]; if(!w->valid) return 0;
    int lr= v->loops>0 && r->rev>=8 && w->loop_end>w->loop_start;   /* wave buffer v2: loop region */
    i32 from= lr? w->loop_start : w->start, to= lr? w->loop_end : w->end, pos=from+v->off;
    if(pos>=to){
      if(w->loop && (r->rev<8 || w->loop_count<0 || v->loops<w->loop_count)){
        v->loops++; v->off=0; if(r->rev>=5) v->played=0;
        if(v->fmt==FMT_ADPCM) adpcm_ctx(v,w->ctx);
        if(to<=from) return 0;
        continue; }
      w->valid=0; v->consumed++; v->cur=(v->cur+1)&3; v->off=0; v->loops=0;
      continue; }
    if(v->off==0 && v->fmt==FMT_ADPCM) adpcm_ctx(v,w->ctx);
    const u8* p=GP(w->addr); int n=v->nch;
    switch(v->fmt){
      case FMT_PCM8: for(int c=0;c<n;c++) f[c]=(float)((i8)p[(u64)pos*n+c]*256); break;
      case FMT_PCM16: for(int c=0;c<n;c++){ i16 s; memcpy(&s,p+((u64)pos*n+c)*2,2); f[c]=s; } break;
      case FMT_PCM24: for(int c=0;c<n;c++){ const u8* q=p+((u64)pos*n+c)*3; i32 s=(i32)((u32)q[0]<<8|(u32)q[1]<<16|(u32)q[2]<<24); f[c]=(float)(s>>16); } break;
      case FMT_PCM32: for(int c=0;c<n;c++){ i32 s; memcpy(&s,p+((u64)pos*n+c)*4,4); f[c]=(float)(s>>16); } break;
      case FMT_FLOAT: for(int c=0;c<n;c++){ float s; memcpy(&s,p+((u64)pos*n+c)*4,4); s*=32768.0f; f[c]= s>32767.0f? 32767.0f : s<-32768.0f? -32768.0f : s; } break;
      case FMT_ADPCM: {   /* 8-byte frames: a predictor/scale byte, then 14 4-bit samples */
        const u8* fr=p+(u64)(pos/14)*8; int k=pos%14, hdr=fr[0], b=fr[1+k/2], nib= (k&1)? (b&15) : (b>>4);
        if(nib>=8) nib-=16;
        int ci=(hdr>>4)&7; i32 s=(nib*(1<<(hdr&15))*2048 + 1024 + v->coef[ci*2]*v->yn1 + v->coef[ci*2+1]*v->yn2)>>11;
        if(s>32767) s=32767; if(s<-32768) s=-32768;
        v->yn2=v->yn1; v->yn1=s; f[0]=(float)s; break; }
      default: return 0; }
    v->off++; v->played++; return 1; }
  return 0; }

/* one period of a voice at the renderer rate into r->vo, with its biquads and volume ramp */
static void voice_render(AudRen* r, Voice* v){
  int n=(int)r->count, nch=v->nch; float f[6];
  double step=(double)v->rate*v->pitch/(double)r->rate; if(!(step>0.0)) step=1.0; if(step>64.0) step=64.0;
  for(int t=0;t<n;t++){
    for(int c=0;c<nch;c++) r->vo[c][t]=v->s0[c]+(v->s1[c]-v->s0[c])*(float)v->frac;
    v->frac+=step;
    while(v->frac>=1.0){ v->frac-=1.0; memcpy(v->s0,v->s1,sizeof v->s0);
      if(next_frame(r,v,f)) memcpy(v->s1,f,sizeof(float)*nch); else memset(v->s1,0,sizeof v->s1); } }
  for(int k=0;k<2;k++) if(v->bq[k].on) for(int c=0;c<nch;c++){
    float b0=v->bq[k].b0, b1=v->bq[k].b1, b2=v->bq[k].b2, a1=v->bq[k].a1, a2=v->bq[k].a2, *s=v->bqs[c][k];
    for(int t=0;t<n;t++){ float x=r->vo[c][t], y=b0*x+s[0]; s[0]=s[1]+b1*x+a1*y; s[1]=b2*x+a2*y; r->vo[c][t]=y; } }
  float g0=v->vol0, dg=(v->vol-v->vol0)/(float)n;
  for(int c=0;c<nch;c++) for(int t=0;t<n;t++) r->vo[c][t]*=g0+dg*(float)(t+1);
  v->vol0=v->vol; }

static float* mixbuf(AudRen* r, const Mix* m, int b){ return r->buf+(size_t)(m->base+b)*r->count; }
static int mix_ok(AudRen* r, u32 id){ return id<r->nmix && r->mix[id].in_use && r->mix[id].nbuf>0; }
static void mix_into(float* dst, const float* src, float g, int n){ if(g!=0.0f) for(int t=0;t<n;t++) dst[t]+=src[t]*g; }
static float clampf(float v, float lo, float hi){ return !(v>=lo)? lo : v>hi? hi : v; }

/* reverb of one channel: out = in*dry + tail*wet (in and out may be the same buffer) */
static void verb_run(Verb* vb, int c, const float* in, float* out, int n, float dry, float wet, float t60, float damp){
  float fb[4]; for(int k=0;k<4;k++) fb[k]=clampf(powf(10.0f,-3.0f*(float)VERB_COMB[k]/(t60*48000.0f)),0.0f,0.98f);
  int sp= (c&1)? 23 : 0;   /* a little stereo spread */
  for(int t=0;t<n;t++){ float x=in[t], acc=0;
    for(int k=0;k<4;k++){ float* b=vb->line[c][k]; int* p=&vb->pos[c][k]; float y=b[*p];
      vb->lp[c][k]=y*(1.0f-damp)+vb->lp[c][k]*damp; b[*p]=x+vb->lp[c][k]*fb[k]; if(++*p>=VERB_COMB[k]+sp) *p=0; acc+=y*(1.0f-fb[k]); }   /* unity gain whatever the decay */
    acc*=0.25f;
    for(int k=0;k<2;k++){ float* b=vb->line[c][4+k]; int* p=&vb->pos[c][4+k]; float bo=b[*p];
      b[*p]=acc+bo*0.5f; acc=bo-acc; if(++*p>=VERB_AP[k]+sp) *p=0; }
    out[t]=x*dry+acc*wet; } }
static Verb* verb_get(Effect* e){
  if(e->verb) return e->verb;
  Verb* v=calloc(1,sizeof *v);
  for(int c=0;c<6;c++){ for(int k=0;k<4;k++) v->line[c][k]=calloc(VERB_COMB[k]+23,sizeof(float)); for(int k=0;k<2;k++) v->line[c][4+k]=calloc(VERB_AP[k]+23,sizeof(float)); }
  return e->verb=v; }
static void verb_free(Verb* v){ if(!v) return; for(int c=0;c<6;c++) for(int k=0;k<6;k++) free(v->line[c][k]); free(v); }

/* a mix's effects, in the game's processing order. Each maps input buffers of the mix to output buffers */
static void effects_run(AudRen* r, u32 mixid){
  Mix* m=&r->mix[mixid]; int n=(int)r->count;
  i64 last=-1;
  for(;;){
    Effect* e=NULL; i64 best=0;
    for(u32 i=0;i<r->neff;i++){ Effect* f=&r->ef[i]; if(f->mix!=mixid || !f->type) continue;
      i64 key=(i64)f->order*4096+i; if(key>last && (!e || key<best)){ best=key; e=f; } }
    if(!e) break;
    last=best;
    if(!e->on) continue;
    const u8* p=e->sp; const i8* in=(const i8*)p; const i8* out=(const i8*)p+6;
    #define BUF(i) ((i)>=0 && (i)<m->nbuf? mixbuf(r,m,(i)) : NULL)
    switch(e->type){
      case 1: { u32 cnt=rd32(p+144); if(cnt>24) cnt=24;   /* buffer mixer */
        for(u32 k=0;k<cnt;k++){ float* a=BUF((i8)p[k]); float* b=BUF((i8)p[24+k]); if(a && b) mix_into(b,a,rdf(p+48+k*4),n); } break; }
      case 6: { int nc=(i8)p[0x16]; if(nc>6) nc=6; i16 cf[5]; memcpy(cf,p+0xC,10);   /* biquad filter, Q14 */
        float b0=cf[0]/16384.0f, b1=cf[1]/16384.0f, b2=cf[2]/16384.0f, a1=cf[3]/16384.0f, a2=cf[4]/16384.0f;
        for(int c=0;c<nc;c++){ float* x=BUF(in[c]); float* y=BUF(out[c]); if(!x || !y) continue; float* s=e->bq[c];
          for(int t=0;t<n;t++){ float v=x[t], o=b0*v+s[0]; s[0]=s[1]+b1*v+a1*o; s[1]=b2*v+a2*o; y[t]=o; } } break; }
      case 7: { u16 nc16; memcpy(&nc16,p+0xE,2); int nc= nc16>6? 6 : nc16;   /* light limiter */
        float atk=clampf(rdf(p+0x24),0,1), rel=clampf(rdf(p+0x28),0,1), th=rdf(p+0x2C), gi=rdf(p+0x30), go=rdf(p+0x34);
        int la=(int)rd32(p+0x38); if(la<1) la=1; if(la>64) la=64;
        if(!(th>0.0f)) th=1.0f; if(!(gi>0.0f) || gi>16) gi=1; if(!(go>0.0f) || go>16) go=1;
        for(int c=0;c<nc;c++){ float* x=BUF(in[c]); float* y=BUF(out[c]); if(!x || !y) continue;
          if(e->att[c]==0.0f) e->att[c]=1.0f;
          for(int t=0;t<n;t++){ float v=x[t]/32768.0f*gi, a=fabsf(v);
            e->env[c]+=(a>e->env[c]? atk : rel)*(a-e->env[c]);
            float g= e->env[c]>th? th/e->env[c] : 1.0f;
            e->att[c]+=(g<e->att[c]? atk : rel)*(g-e->att[c]);
            float d=e->la[c][e->lapos[c]]; e->la[c][e->lapos[c]]=v; e->lapos[c]=(e->lapos[c]+1)%la;
            y[t]=d*e->att[c]*go*32768.0f; } } break; }
      case 5: { u16 nc16; memcpy(&nc16,p+0xE,2); int nc= nc16>6? 6 : nc16;   /* I3DL2 reverb: gains in millibels */
        float t60=clampf(rdf(p+0x20),0.1f,20.0f), hf=clampf(rdf(p+0x24),0.1f,2.0f), room=rdf(p+0x28), rev=rdf(p+0x30), dry=clampf(rdf(p+0x44),0,1);
        float wet=clampf(powf(10.0f,clampf(room+rev,-10000.0f,5000.0f)/2000.0f),0,1); Verb* vb=verb_get(e);
        for(int c=0;c<nc;c++){ float* x=BUF(in[c]); float* y=BUF(out[c]); if(x && y) verb_run(vb,c,x,y,n,dry,wet,t60,clampf(0.7f-0.35f*hf,0,0.7f)); } break; }
      case 4: { u16 nc16; memcpy(&nc16,p+0xE,2); int nc= nc16>6? 6 : nc16;   /* reverb: Q14 */
        float t60=clampf((i32)rd32(p+0x28)/16384.0f,0.1f,20.0f), hf=clampf((i32)rd32(p+0x2C)/16384.0f,0.1f,2.0f);
        float wet=clampf((i32)rd32(p+0x38)/16384.0f,0,1), dry=clampf((i32)rd32(p+0x3C)/16384.0f,0,1); Verb* vb=verb_get(e);
        for(int c=0;c<nc;c++){ float* x=BUF(in[c]); float* y=BUF(out[c]); if(x && y) verb_run(vb,c,x,y,n,dry,wet,t60,clampf(0.7f-0.35f*hf,0,0.7f)); } break; }
      default: break;   /* aux, delay, capture, compressor: the sound passes through */
    }
    #undef BUF
  } }

static void ren_render(AudRen* r){
  int n=(int)r->count;
  memset(r->buf,0,sizeof(float)*r->nmixbuf*r->count);
  /* voices into the mixes they feed, directly or through a splitter */
  for(u32 i=0;i<r->nvoice;i++){ Voice* v=&r->v[i];
    if(!v->in_use || v->state!=0) continue;
    int direct=mix_ok(r,v->mix), split= v->split>=0 && (u32)v->split<r->nsplit;
    if(!direct && !split) continue;
    voice_render(r,v);
    if(direct){ Mix* m=&r->mix[v->mix];
      for(int c=0;c<v->nch;c++){ if(v->res[c]>=r->nvoice) continue; ChanRes* cr=&r->cr[v->res[c]]; if(!cr->in_use) continue;
        for(int b=0;b<m->nbuf && b<24;b++){ float g0=cr->vol0[b], dg=(cr->vol[b]-g0)/(float)n; if(g0==0.0f && dg==0.0f) continue;
          float* d=mixbuf(r,m,b); for(int t=0;t<n;t++) d[t]+=r->vo[c][t]*(g0+dg*(float)(t+1)); } } }
    else { Split* s=&r->sp[v->split];
      for(int j=0;j<s->n;j++){ i32 di=s->dst[j]; if(di<0 || (u32)di>=r->nsdst) continue; SplitDst* d=&r->sd[di]; if(!d->in_use || !mix_ok(r,d->mix)) continue;
        Mix* m=&r->mix[d->mix]; int c=j%v->nch; for(int b=0;b<m->nbuf && b<24;b++) mix_into(mixbuf(r,m,b),r->vo[c],d->vol[b],n); } } }
  for(u32 i=0;i<r->nvoice;i++) memcpy(r->cr[i].vol0,r->cr[i].vol,sizeof r->cr[i].vol);
  /* each submix through its effects into where it goes, the deepest first */
  for(i32 k=0;k<r->norder;k++){ Mix* m=&r->mix[r->order[k]]; effects_run(r,(u32)r->order[k]);
    if((u32)m->dst!=UNUSED_MIX && mix_ok(r,(u32)m->dst)){ Mix* d=&r->mix[m->dst];
      for(int a=0;a<m->nbuf && a<24;a++) for(int b=0;b<d->nbuf && b<24;b++) mix_into(mixbuf(r,d,b),mixbuf(r,m,a),m->volume*m->mv[a][b],n); }
    else if(m->dsplit>=0 && (u32)m->dsplit<r->nsplit){ Split* s=&r->sp[m->dsplit];
      for(int j=0;j<s->n;j++){ i32 di=s->dst[j]; if(di<0 || (u32)di>=r->nsdst) continue; SplitDst* d=&r->sd[di]; if(!d->in_use || !mix_ok(r,d->mix)) continue;
        Mix* t=&r->mix[d->mix]; for(int b=0;b<t->nbuf && b<24;b++) mix_into(mixbuf(r,t,b),mixbuf(r,m,j%m->nbuf),m->volume*d->vol[b],n); } } }
  /* final mix volume, then the sinks */
  float out[2][PERIOD]; memset(out,0,sizeof out);
  if(mix_ok(r,0)){ Mix* f=&r->mix[0]; effects_run(r,0);
    if(f->volume!=1.0f) for(int b=0;b<f->nbuf;b++){ float* d=mixbuf(r,f,b); for(int t=0;t<n;t++) d[t]*=f->volume; }
    for(u32 s=0;s<r->nsink;s++){ Sink* k=&r->sk[s]; if(!k->in_use) continue;
      const float* ch[6]; u32 nin= k->nin>6? 6 : k->nin; int ok=1;
      for(u32 c=0;c<nin;c++){ if(k->in[c]<0 || k->in[c]>=f->nbuf){ ok=0; break; } ch[c]=mixbuf(r,f,k->in[c]); }
      if(!ok || !nin) continue;
      if(k->type==1){   /* device: 6 channels are FL FR FC LFE BL BR */
        if(nin>=6){ static const float def[4]={1.0f,0.707f,0.251f,0.707f}; const float* q= k->downmix? k->coef : def;
          for(int t=0;t<n;t++){ float cm=ch[2][t]*q[1]+ch[3][t]*q[2]; out[0][t]+=ch[0][t]*q[0]+cm+ch[4][t]*q[3]; out[1][t]+=ch[1][t]*q[0]+cm+ch[5][t]*q[3]; } }
        else for(int t=0;t<n;t++){ out[0][t]+=ch[0][t]; out[1][t]+=ch[nin>1? 1 : 0][t]; } }
      else if(k->type==2 && k->size>=(u32)n*2 && mapped(k->addr,k->size)){   /* circular buffer: PCM16, a block per channel */
        for(u32 c=0;c<nin;c++){ if(k->pos+(u32)n*2>k->size) k->pos=0;
          i16* d=(i16*)GP(k->addr+k->pos); for(int t=0;t<n;t++){ float x=ch[c][t]; d[t]=(i16)(x>32767.0f? 32767 : x<-32768.0f? -32768 : (int)x); }
          k->pos+=(u32)n*2; if(k->pos>=k->size) k->pos=0; } } } }
  /* to 48 kHz (renderers can run at 32 kHz, 160 samples a period) */
  if(n==PERIOD) memcpy(r->o48,out,sizeof out);
  else for(int c=0;c<2;c++) for(int t=0;t<PERIOD;t++){ double p=(double)(t+1)*n/PERIOD-1.0; int i=(int)p; float fr=(float)(p-i);
    if(p<0){ i=-1; fr=(float)(p+1.0); } float a= i<0? r->rs_prev[c] : out[c][i], b=out[c][i+1<n? i+1 : n-1]; r->o48[c][t]=a+(b-a)*fr; }
  r->rs_prev[0]=out[0][n-1]; r->rs_prev[1]=out[1][n-1];
  r->frames++; }

/* submixes in the order they flow: the ones furthest from the final mix first */
static void ren_order(AudRen* r){
  for(u32 i=0;i<r->nmix;i++) r->mix[i].depth= i==0? 0 : -1;
  for(u32 pass=0;pass<r->nmix;pass++){ int changed=0;
    for(u32 i=1;i<r->nmix;i++){ Mix* m=&r->mix[i]; if(!m->in_use) continue; i32 d=-1;
      if((u32)m->dst!=UNUSED_MIX && (u32)m->dst<r->nmix && r->mix[m->dst].depth>=0) d=r->mix[m->dst].depth+1;
      else if(m->dsplit>=0 && (u32)m->dsplit<r->nsplit){ Split* s=&r->sp[m->dsplit];
        for(int j=0;j<s->n;j++){ i32 di=s->dst[j]; if(di<0 || (u32)di>=r->nsdst || !r->sd[di].in_use) continue; u32 t=r->sd[di].mix;
          if(t<r->nmix && r->mix[t].depth>=0 && r->mix[t].depth+1>d) d=r->mix[t].depth+1; } }
      if(d>(i32)r->nmix) d=(i32)r->nmix;
      if(d!=m->depth){ m->depth=d; changed=1; } }
    if(!changed) break; }
  r->norder=0;
  for(i32 d=(i32)r->nmix;d>=1;d--) for(u32 i=1;i<r->nmix;i++) if(r->mix[i].in_use && r->mix[i].depth==d) r->order[r->norder++]=(i32)i; }

/* mix buffers: the final mix first, then the submixes in id order */
static void ren_bases(AudRen* r){
  i32 base=0;
  for(u32 i=0;i<r->nmix;i++){ Mix* m=&r->mix[i]; if(!m->in_use) continue;
    if(m->nbuf<0) m->nbuf=0; if(m->nbuf>24) m->nbuf=24;
    if(base+m->nbuf>(i32)r->nmixbuf){ m->nbuf=0; continue; }
    m->base=base; base+=m->nbuf; } }

static void voice_stop(Voice* v){
  for(int i=0;i<4;i++){ v->wb[i].pending=0; if(v->wb[i].valid){ v->cur=(v->cur+1)&3; v->consumed++; } v->wb[i].valid=0; }
  v->off=0; v->played=0; v->frac=0; v->loops=0; v->yn1=v->yn2=0;
  memset(v->s0,0,sizeof v->s0); memset(v->s1,0,sizeof v->s1); memset(v->bqs,0,sizeof v->bqs); }

/* RequestUpdate. Header (in and out): revision, behaviour, memory pools, voices, voice channel resources,
   effects, mixes, sinks, performance, -, render info, pad, total size. The input sections come in the
   order behaviour, pools, channel resources, voices, effects, splitters, mixes, sinks, performance. */
int audren_update(SvcObj* o, IpcCtx* c){
  AudRen* r=o->data;
  u64 ia,is,oa,os; buf_in(c,0,&ia,&is); buf_out(c,0,&oa,&os);
  if(!ia || !oa || is<0x40) return 0;
  u8* in=GP(ia); u8* out=GP(oa);
  u32 ih[16]; memcpy(ih,in,64);
  u32 rev=ih[0]; int revn=(int)((rev>>24)&0xFF)-'0'; if(revn<0) revn=0;
  u32 behav=ih[1], mp=ih[2], vo=ih[3], vr=ih[4], ef=ih[5], mx=ih[6], sk=ih[7], pf=ih[8];
  u32 npools=mp/0x20, nvoices=vo/0x170, nres=vr/0x70, neff=ef/0xC0, nsinks=sk/0x140;
  u64 fixed=0x40ull+behav+mp+vr+vo+ef+mx+sk+pf;
  if(fixed>is){ WARN("audren: update data larger than its buffer\n"); return RC(153,41); }
  u8* p=in+0x40+behav;
  u8* pools_in=p; p+=mp; u8* res_in=p; p+=vr; u8* voices_in=p; p+=vo; u8* eff_in=p; p+=ef;
  u32 spsz= ih[15]>fixed && ih[15]<=is? (u32)(ih[15]-fixed) : 0; u8* split_in=p; p+=spsz;
  u8* mix_in=p; p+=mx; u8* sink_in=p;
  int eff_v2 = revn>=9;
  u32 o_mp=npools*0x10, o_vo=nvoices*0x10, o_ef=neff*(eff_v2? 0x90 : 0x10), o_sk=nsinks*0x20, o_pf=0x10, o_bh=0xB0, o_ri= revn>=5? 0x10 : 0;
  u32 oh[16]; memset(oh,0,sizeof oh);
  oh[0]=rev; oh[1]=o_bh; oh[2]=o_mp; oh[3]=o_vo; oh[4]=0; oh[5]=o_ef; oh[6]=0; oh[7]=o_sk; oh[8]=o_pf; oh[9]=0; oh[10]=o_ri;
  oh[15]=0x40+o_mp+o_vo+o_ef+o_sk+o_pf+o_bh+o_ri;
  if(oh[15]>os){ WARN("audren: output buffer too small (%u > %llu)\n",oh[15],(unsigned long long)os); return RC(153,4); }
  { static int said; if(!said){ said=1; LOG("audren: REV%d, %u pools, %u voices, %u effects, %u sinks, %u mix buffers\n",revn,npools,nvoices,neff,nsinks,r->nmixbuf); } }
  memset(out,0,oh[15]); memcpy(out,oh,64);
  u8* q=out+0x40;
  pm_lock(&r->lk);
  r->rev=revn;
  /* pools: RequestAttach(4)->Attached(5), RequestDetach(2)->Detached(3). The DSP reads guest memory directly */
  for(u32 i=0;i<npools;i++){ u32 st=rd32(pools_in+i*0x20+0x10); u32 ns= st==4? 5 : st==2? 3 : st; memcpy(q+i*0x10,&ns,4); }
  q+=o_mp;
  /* voice channel resources, by position: the volumes from each voice channel into the mix buffers */
  for(u32 i=0;i<nres && i<r->nvoice;i++){ const u8* e=res_in+i*0x70; ChanRes* cr=&r->cr[i];
    int was=cr->in_use; cr->in_use=e[0x64];
    if(cr->in_use){ memcpy(cr->vol,e+4,sizeof cr->vol); if(!was) memcpy(cr->vol0,cr->vol,sizeof cr->vol); } }
  /* voices: only the entries in use count (the others still carry an id, often 0) */
  for(u32 i=0;i<r->nvoice;i++) r->v[i].in_use=0;
  for(u32 i=0;i<nvoices;i++){ const u8* vi=voices_in+i*0x170; u8* vout=q+i*0x10;
    u32 id=rd32(vi); if(!vi[9] || id>=r->nvoice) continue; Voice* v=&r->v[id];
    if(vi[8]){ memset(v,0,sizeof *v); v->state=1; }   /* is_new */
    v->in_use=1;
    {
      v->fmt=vi[0xB]; v->rate=rd32(vi+0xC); if(!v->rate || v->rate>192000) v->rate=48000;
      u32 nch=rd32(vi+0x18); v->nch=(u8)(nch<1? 1 : nch>6? 6 : nch); if(v->fmt==FMT_ADPCM) v->nch=1;
      v->pitch=rdf(vi+0x1C); if(!(v->pitch>0.0f) || v->pitch>16.0f) v->pitch=1.0f;
      float vol=rdf(vi+0x20); if(!(vol>=0.0f) || vol>16.0f) vol=0.0f; v->vol=vol; if(vi[8]) v->vol0=vol;
      for(int k=0;k<2;k++){ const u8* b=vi+0x24+k*0xC; i16 cf[5]; memcpy(cf,b+2,10); int on=b[0]!=0;
        if(on && !v->bq[k].on) for(int c=0;c<6;c++) v->bqs[c][k][0]=v->bqs[c][k][1]=0;
        v->bq[k].on=(u8)on; v->bq[k].b0=cf[0]/16384.0f; v->bq[k].b1=cf[1]/16384.0f; v->bq[k].b2=cf[2]/16384.0f; v->bq[k].a1=cf[3]/16384.0f; v->bq[k].a2=cf[4]/16384.0f; }
      u64 src; memcpy(&src,vi+0x48,8);
      if(v->fmt==FMT_ADPCM && mapped(src,0x20)) memcpy(v->coef,GP(src),0x20);
      v->mix=rd32(vi+0x58); v->split=(i32)rd32(vi+0x5C);
      for(int ch=0;ch<6;ch++) v->res[ch]=rd32(vi+0x140+ch*4);
      /* wave buffers the game hasn't sent before (sent_to_DSP == 0) */
      for(int w=0;w<4;w++){ const u8* wb=vi+0x60+w*0x38; if(wb[0x1A]) continue;
        WBuf* d=&v->wb[w]; u64 addr, size, ctx; memcpy(&addr,wb,8); memcpy(&size,wb+8,8); memcpy(&ctx,wb+0x20,8);
        i32 st=(i32)rd32(wb+0x10), en=(i32)rd32(wb+0x14);
        if(st<0) st=0; if(en<st) en=st;
        if(!mapped(addr,size)){ if(addr) { static int nw; if(nw++<8) WARN("audren: voice %u wave buffer %llx (%llu bytes) is not in mapped memory\n",id,(unsigned long long)addr,(unsigned long long)size); } continue; }
        while(en>st && wb_bytes(v->fmt,v->nch,en)>size) en--;
        d->addr=addr; d->size=size; d->start=st; d->end=en; d->loop=wb[0x18]; d->loop_count=(i32)rd32(wb+0x1C);
        d->ctx= mapped(ctx,6)? ctx : 0; d->loop_start=(i32)rd32(wb+0x30); d->loop_end=(i32)rd32(wb+0x34);
        if(d->loop_start<0) d->loop_start=0; if(d->loop_end>en) d->loop_end=en;
        d->pending=1; }
      /* flush: drop buffers from the game's current index without playing them */
      { int idx=*(const u16*)(vi+0x40)&3; for(int k=0;k<vi[0x159] && k<4;k++){ v->wb[idx].pending=0;
          if(v->wb[idx].valid && v->cur==idx){ v->cur=(v->cur+1)&3; v->consumed++; v->off=0; v->loops=0; } v->wb[idx].valid=0; idx=(idx+1)&3; } }
      u8 ps=vi[0xA];
      if(ps==0){ v->state=0; for(int w=0;w<4;w++) if(v->wb[w].pending){ v->wb[w].pending=0; v->wb[w].valid=1; } }
      else if(ps==2) v->state=2;
      else if(v->state!=1){ voice_stop(v); v->state=1; } }
    memcpy(vout,&v->played,8); memcpy(vout+8,&v->consumed,4); }
  q+=o_vo;
  /* effects: Enabled(3) / Disabled(4) */
  for(u32 i=0;i<neff;i++){ const u8* e=eff_in+i*0xC0; q[i*(eff_v2? 0x90 : 0x10)]= e[2]? 3 : 4;
    if(i<r->neff){ Effect* d=&r->ef[i];
      if(e[1] || d->type!=e[0]){ verb_free(d->verb); memset(d,0,sizeof *d); }   /* is_new or another effect: fresh state */
      d->type=e[0]; d->on=e[2]; d->mix=rd32(e+4); d->order=rd32(e+0x18); memcpy(d->sp,e+0x20,0xA0); } }
  q+=o_ef;
  /* splitters: a header ('SNDH', splitter count, destination count), the splitters ('SNDI', id, rate,
     count, destination ids at +0x10), the destinations ('SNDD', id, 24 volumes, mix id, in use).
     Entries grow with the revision (a splitter is 0x1C + 4 per id on REV11), so each next one is
     found by its magic */
  if(spsz>=0x20 && rd32(split_in)==0x48444E53){
    u32 ni=rd32(split_in+4), nd=rd32(split_in+8); u32 off=0x20;
    #define NEXT_MAGIC(m) while(off+4<=spsz && rd32(split_in+off)!=(m)) off+=4
    for(u32 k=0;k<ni && off+0x10<=spsz;k++){ const u8* e=split_in+off; if(rd32(e)!=0x49444E53) break;
      i32 sid=(i32)rd32(e+4); u32 cnt=rd32(e+0xC); if(cnt>spsz/4 || off+0x10+cnt*4>spsz) break;
      if(sid>=0 && (u32)sid<r->nsplit){ Split* s=&r->sp[sid]; s->n= cnt>48? 48 : (i32)cnt; for(i32 j=0;j<s->n;j++) s->dst[j]=(i32)rd32(e+0x10+j*4); }
      off+=0x10+cnt*4;
      if(k+1<ni) NEXT_MAGIC(0x49444E53); else if(nd) NEXT_MAGIC(0x44444E53); }
    for(u32 k=0;k<nd && off+0x70<=spsz;k++){ const u8* e=split_in+off; if(rd32(e)!=0x44444E53) break;
      i32 did=(i32)rd32(e+4);
      if(did>=0 && (u32)did<r->nsdst){ SplitDst* d=&r->sd[did]; memcpy(d->vol,e+8,sizeof d->vol); d->mix=rd32(e+0x68); d->in_use=e[0x6C]; }
      off+=0x70; if(k+1<nd) NEXT_MAGIC(0x44444E53); }
    #undef NEXT_MAGIC
  }
  /* mixes: all of them by index, or since REV7 only the changed ones behind a 0x20 header */
  { u32 nm=mx/0x930, at=0; int dirty= mx%0x930==0x20; if(dirty){ nm=(mx-0x20)/0x930; at=0x20; }
    for(u32 k=0;k<nm;k++){ const u8* e=mix_in+at+k*0x930; u32 id= dirty? rd32(e+0x10) : k; if(id>=r->nmix) continue;
      Mix* m=&r->mix[id]; m->in_use=e[0xC]; m->volume=rdf(e); m->nbuf=(i32)rd32(e+8);
      memcpy(m->mv,e+0x24,sizeof m->mv); m->dst=(i32)rd32(e+0x924); m->dsplit=(i32)rd32(e+0x928); }
    ren_bases(r); ren_order(r); }
  /* sinks: device (1) or circular buffer (2) */
  for(u32 i=0;i<nsinks;i++){ const u8* e=sink_in+i*0x140; u8* so=q+i*0x20; if(i>=r->nsink) continue;
    Sink* s=&r->sk[i]; s->type=e[0]; s->in_use=e[1];
    if(s->type==1){ s->nin=rd32(e+0x120); memcpy(s->in,e+0x124,6); s->downmix=e[0x12B]; memcpy(s->coef,e+0x12C,16); }
    else if(s->type==2){ u64 a; memcpy(&a,e+0x20,8); u32 sz=rd32(e+0x28); if(a!=s->addr || sz!=s->size) s->pos=0;
      s->addr=a; s->size=sz; s->nin=rd32(e+0x2C); memcpy(s->in,e+0x39,6); memcpy(so,&s->pos,4); } }
  q+=o_sk; q+=o_pf; q+=o_bh;
  if(o_ri) memcpy(q,&r->frames,8);
  pm_unlock(&r->lk);
  return 0;
}

static void audren_destroy(SvcObj* o){
  AudRen* r=o->data;
  pm_lock(&g_alk); for(AudRen** pp=&g_rens;*pp;pp=&(*pp)->next) if(*pp==r){ *pp=r->next; break; } pm_unlock(&g_alk);
  for(u32 i=0;i<r->neff;i++) verb_free(r->ef[i].verb);
  free(r->v); free(r->cr); free(r->mix); free(r->sk); free(r->ef); free(r->sp); free(r->sd); free(r->buf); free(r->order); }

static int audren_obj(SvcObj* o, IpcCtx* c){
  AudRen* r=o->data;
  switch(c->cmd){
    case 0: out32(c,r->rate); return 0;
    case 1: out32(c,r->count); return 0;
    case 2: out32(c,r->nmixbuf); return 0;
    case 3: out32(c,r->started? 0 : 1); return 0;
    case 4: case 10: return audren_update(o,c);
    case 5: pm_lock(&r->lk); r->started=1; pm_unlock(&r->lk); return 0;
    case 6: pm_lock(&r->lk); r->started=0; pm_unlock(&r->lk); return 0;
    case 7: out_copy_h(c,h_new(&g_ren_ev->o)); return 0;
    case 8: r->limit=in32(c,0); return 0;
    case 9: out32(c,r->limit); return 0;
  }
  return 0; }

static int auddev_disp(SvcObj* o, IpcCtx* c){ (void)o;
  static const char* nm="AudioTvOutput";
  switch(c->cmd){
    case 0: case 6: { u64 a,s; if(buf_out(c,0,&a,&s) && a && s>=0x100){ memset(GP(a),0,0x100); strcpy((char*)GP(a),nm); out32(c,1); } else out32(c,0); return 0; }
    case 2: case 8: out32(c,0x3F800000); return 0;
    case 3: case 10: case 13: { u64 a,s; if(buf_out(c,0,&a,&s) && a){ memset(GP(a),0,s); strncpy((char*)GP(a),nm,s); } return 0; }
    case 4: case 11: case 12: { KEvent* e=kevent_new("audio device",0); e->signaled=1; out_copy_h(c,h_new(&e->o)); return 0; }
    case 5: out32(c,2); return 0;
  }
  return 0; }

static u32 cap(u32 v, u32 lo, u32 hi){ return v<lo? lo : v>hi? hi : v; }
static int audren_disp(SvcObj* o, IpcCtx* c){ (void)o;
  switch(c->cmd){
    case 0: {   /* OpenAudioRenderer: rate, sample count, mix buffers, submixes, voices, sinks, effects, ..., splitters, destinations */
      SvcObj* so=svcobj_new("IAudioRenderer",audren_obj,sizeof(AudRen)); AudRen* r=so->data;
      pm_init(&r->lk); r->rate=in32(c,0)==32000? 32000 : 48000; r->count=cap(in32(c,4),1,PERIOD); r->limit=100;
      r->nmixbuf=cap(in32(c,8),1,1024); r->nmix=cap(in32(c,0xC)+1,1,512); r->nvoice=cap(in32(c,0x10),1,1024);
      r->nsink=cap(in32(c,0x14),1,32); r->neff=cap(in32(c,0x18),1,512); r->nsplit=cap(in32(c,0x24),1,1024); r->nsdst=cap(in32(c,0x28),1,4096);
      r->v=calloc(r->nvoice,sizeof(Voice)); r->cr=calloc(r->nvoice,sizeof(ChanRes)); r->mix=calloc(r->nmix,sizeof(Mix));
      r->sk=calloc(r->nsink,sizeof(Sink)); r->ef=calloc(r->neff,sizeof(Effect)); r->sp=calloc(r->nsplit,sizeof(Split));
      r->sd=calloc(r->nsdst,sizeof(SplitDst)); r->buf=calloc((size_t)r->nmixbuf*r->count,sizeof(float)); r->order=calloc(r->nmix,sizeof(i32));
      for(u32 i=0;i<r->nvoice;i++) r->v[i].state=1;
      for(u32 i=0;i<r->nmix;i++){ r->mix[i].dst=UNUSED_MIX; r->mix[i].dsplit=-1; }
      so->destroy=audren_destroy;
      pm_lock(&g_alk); r->next=g_rens; g_rens=r; pm_unlock(&g_alk); g_want_host=1;
      LOG("audren: renderer open, %u Hz, %u voices, %u mix buffers, %u submixes, %u sinks\n",r->rate,r->nvoice,r->nmixbuf,r->nmix-1,r->nsink);
      out_obj(c,so); return 0; }
    case 1: { u64 sz=0x4000; u32 voices=in32(c,0x10), sinks=in32(c,0x14), effects=in32(c,0x1C); sz+= (u64)voices*0x400 + sinks*0x200 + effects*0x800 + 0x40000; out64(c,sz); return 0; }
    case 2: case 4: out_obj(c,svcobj_new("IAudioDevice",auddev_disp,0)); return 0;
  }
  return 0; }
SvcObj* new_audren(void){ return svcobj_new("audren:u",audren_disp,0); }

/* ============================================================ audout: PCM16 buffers played in order */
typedef struct { u64 tag, data, size, done; } AoBuf;
typedef struct AudOut {
  int started; u32 nch; float vol;
  AoBuf q[32]; int qh, qn;
  u64 rel[32]; int nrel;
  KEvent* ev; u64 played;
  struct AudOut* next;
} AudOut;
static AudOut* g_outs;

static void aout_release(AudOut* a){
  if(a->nrel<32) a->rel[a->nrel++]=a->q[a->qh].tag;
  a->qh=(a->qh+1)&31; a->qn--; kevent_signal(a->ev); }
static void aout_render(AudOut* a, float* l, float* rt){
  int need=PERIOD, t=0; u32 fb=a->nch*2;
  while(need>0 && a->qn){ AoBuf* b=&a->q[a->qh];
    u64 avail=(b->size-b->done)/fb; if(!avail){ aout_release(a); continue; }
    int take= avail<(u64)need? (int)avail : need; const i16* s=(const i16*)GP(b->data+b->done);
    for(int k=0;k<take;k++,t++){ const i16* f=s+(size_t)k*a->nch; float x, y;
      if(a->nch>=6){ float cm=f[2]*0.707f+f[3]*0.251f; x=f[0]+cm+f[4]*0.707f; y=f[1]+cm+f[5]*0.707f; }
      else if(a->nch>=2){ x=f[0]; y=f[1]; } else x=y=f[0];
      l[t]+=x*a->vol; rt[t]+=y*a->vol; }
    b->done+=(u64)take*fb; need-=take; a->played+=(u64)take;
    if(b->done+fb>b->size) aout_release(a); } }

static int audout_obj(SvcObj* o, IpcCtx* c){
  AudOut* a=o->data; int r=0;
  pm_lock(&g_alk);
  switch(c->cmd){
    case 0: out32(c,a->started? 0 : 1); break;
    case 1: a->started=1; break;
    case 2: a->started=0; break;
    case 3: case 7: {   /* AppendAudioOutBuffer(tag, {next, data, capacity, size, offset}) */
      u64 ba,bs; buf_in(c,0,&ba,&bs); u64 tag=in64(c,0);
      if(!ba || bs<0x28 || !mapped(ba,0x28)) { r=RC(153,41); break; }
      u64 f[5]; memcpy(f,GP(ba),0x28);
      if(a->qn>=32){ r=RC(153,8); break; }
      if(!mapped(f[1]+f[4],f[3])){ f[3]=0; }
      AoBuf* b=&a->q[(a->qh+a->qn)&31]; b->tag=tag; b->data=f[1]+f[4]; b->size=f[3]; b->done=0; a->qn++; break; }
    case 4: out_copy_h(c,h_new(&a->ev->o)); break;
    case 5: case 8: {   /* GetReleasedAudioOutBuffers */
      u64 ba,bs; buf_out(c,0,&ba,&bs); u32 n=0;
      if(ba) for(;n<(u32)a->nrel && (n+1)*8<=bs;n++) memcpy(GP(ba+n*8),&a->rel[n],8);
      if(n) { memmove(a->rel,a->rel+n,(a->nrel-n)*8); a->nrel-=(int)n; }
      out32(c,n); break; }
    case 6: { u64 tag=in64(c,0); u8 f=0; for(int i=0;i<a->qn;i++) if(a->q[(a->qh+i)&31].tag==tag) f=1; out8(c,f); break; }
    case 9: out32(c,(u32)a->qn); break;
    case 10: out64(c,a->played); break;
    case 11: { int had=a->qn>0; while(a->qn) aout_release(a); out8(c,(u8)had); break; }
    case 12: a->vol=*(float*)c->raw; if(!(a->vol>=0.0f) || a->vol>8.0f) a->vol=1.0f; break;
    case 13: { float v=a->vol; u32 u; memcpy(&u,&v,4); out32(c,u); break; }
  }
  pm_unlock(&g_alk); return r; }
static void audout_destroy(SvcObj* o){
  AudOut* a=o->data;
  pm_lock(&g_alk); for(AudOut** pp=&g_outs;*pp;pp=&(*pp)->next) if(*pp==a){ *pp=a->next; break; } pm_unlock(&g_alk);
  kobj_unref(&a->ev->o); }
static int audout_disp(SvcObj* o, IpcCtx* c){ (void)o;
  if(c->cmd==0||c->cmd==2){ u64 a,s; if(buf_out(c,0,&a,&s) && a && s>=10) strcpy((char*)GP(a),"DeviceOut"); out32(c,1); return 0; }
  if(c->cmd==1||c->cmd==3){   /* OpenAudioOut({rate, channels}) -> rate, channels, format (PCM16), state (stopped) */
    SvcObj* so=svcobj_new("IAudioOut",audout_obj,sizeof(AudOut)); AudOut* a=so->data;
    a->nch= (in32(c,4)&0xFFFF)==6? 6 : 2; a->vol=1.0f; a->ev=kevent_new("audout buffer",0); kobj_ref(&a->ev->o);
    so->destroy=audout_destroy;
    pm_lock(&g_alk); a->next=g_outs; g_outs=a; pm_unlock(&g_alk); g_want_host=1;
    out32(c,OUT_RATE); out32(c,a->nch); out32(c,2); out32(c,1); out_obj(c,so); return 0; }
  return 0; }
SvcObj* new_audout(void){ return svcobj_new("audout:u",audout_disp,0); }

/* audin: one device, and buffers that never fill (no microphone) */
static int audin_obj(SvcObj* o, IpcCtx* c){ (void)o;
  switch(c->cmd){ case 0: out32(c,1); return 0; case 4: { KEvent* e=kevent_new("audin buffer",0); out_copy_h(c,h_new(&e->o)); return 0; }
    case 5: case 8: out32(c,0); return 0; case 6: out8(c,0); return 0; case 9: out32(c,0); return 0; }
  return 0; }
static int audin_disp(SvcObj* o, IpcCtx* c){ (void)o;
  if(c->cmd==0||c->cmd==2||c->cmd==4){ u64 a,s; if(buf_out(c,0,&a,&s) && a && s>=9) strcpy((char*)GP(a),"DeviceIn"); out32(c,1); return 0; }
  if(c->cmd==1||c->cmd==3||c->cmd==5){ out32(c,OUT_RATE); out32(c,2); out32(c,2); out32(c,1); out_obj(c,svcobj_new("IAudioIn",audin_obj,0)); return 0; }
  return 0; }
static SvcObj* new_audin(void){ return svcobj_new("audin:u",audin_disp,0); }

/* ============================================================ the audio thread */
static void render_period(i16* pcm){
  static float l[PERIOD], rt[PERIOD];
  memset(l,0,sizeof l); memset(rt,0,sizeof rt);
  pm_lock(&g_alk);
  for(AudRen* r=g_rens;r;r=r->next){ pm_lock(&r->lk); int on=r->started; if(on) ren_render(r); pm_unlock(&r->lk);
    if(on) for(int t=0;t<PERIOD;t++){ l[t]+=r->o48[0][t]; rt[t]+=r->o48[1][t]; } }
  for(AudOut* a=g_outs;a;a=a->next) if(a->started) aout_render(a,l,rt);
  pm_unlock(&g_alk);
  int v=g_opt_volume; float g=(float)(v<0? 0 : v>100? 100 : v)/100.0f;
  for(int t=0;t<PERIOD;t++){ float x=l[t]*g, y=rt[t]*g;
    pcm[t*2]=(i16)(x>32767.0f? 32767 : x<-32768.0f? -32768 : (int)x); pcm[t*2+1]=(i16)(y>32767.0f? 32767 : y<-32768.0f? -32768 : (int)y); } }

static void audio_thread(void* arg){ (void)arg;
  plat_thread_high_priority();
  static i16 pcm[PERIOD*2]; int host=0, tried=0; u64 t0=plat_ns(), done=0;
  for(;;){
    /* the sound card opens once a game uses audio, after the window (SDL2 is shared with it on Linux);
       the test runner's headless build has none */
    if(!tried && g_want_host){ tried=1; if(video_has_gl()) host=host_open(); }
    int n;
    if(host) n=host_room();
    else { u64 due=(plat_ns()-t0)/5000000ull; if(due>done+4) done=due-4; n=(int)(due-done); }
    if(n<=0){ if(host) host_wait(); else plat_sleep_ns(1000000); continue; }
    while(n-->0){
      render_period(pcm);
      /* A live sound device does not mean the HLE mixer has nonzero samples.
         Record a five-second peak/activity report to distinguish silent game
         mixes from Windows waveOut failures, without logging every buffer. */
      static u32 periods, audible_periods, peak;
      u32 block_peak=0;
      for(int i=0;i<PERIOD*2;i++){
        int v=pcm[i]; unsigned a=(unsigned)(v<0? -v : v);
        if(a>block_peak) block_peak=a;
      }
      if(block_peak>peak) peak=block_peak;
      if(block_peak>0) audible_periods++;
      if(++periods>=1000){
        u32 renderers=0, running=0, voices=0, sinks=0;
        pm_lock(&g_alk);
        for(AudRen* r=g_rens;r;r=r->next){
          renderers++;
          pm_lock(&r->lk);
          if(r->started){
            running++;
            for(u32 i=0;i<r->nvoice;i++)
              if(r->v[i].in_use && r->v[i].state==0) voices++;
            for(u32 i=0;i<r->nsink;i++)
              if(r->sk[i].in_use && r->sk[i].type==1) sinks++;
          }
          pm_unlock(&r->lk);
        }
        pm_unlock(&g_alk);
        LOG("audio: 5s mixer meter peak=%u/32767 nonzero_periods=%u/1000; renderers=%u started=%u playing_voices=%u device_sinks=%u host=%s volume=%d\n",
            peak,audible_periods,renderers,running,voices,sinks,host?"open":"none",g_opt_volume);
        periods=audible_periods=peak=0;
      }
      if(host) host_write(pcm);
      wav_write(pcm); done++; kevent_signal(g_ren_ev);
    } } }

void audio_init(void){
  pm_init(&g_alk);
  g_ren_ev=kevent_new("audio renderer",0); kobj_ref(&g_ren_ev->o);   /* the thread keeps signalling it */
  plat_thread(audio_thread,NULL,256*1024);
  svc_register("audren:u",new_audren); svc_register("audout:u",new_audout); svc_register("audin:u",new_audin); }
