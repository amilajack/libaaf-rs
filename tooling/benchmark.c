/* Development-only GPL-2.0-or-later C reference harness. */
#include <libaaf/AAFIface.h>
#include <libaaf/AAFIEssenceFile.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
static void quiet(struct aafLog *l,void *c,int b,int t,const char *f,const char *fn,int n,const char *m,void *u){(void)l;(void)c;(void)b;(void)t;(void)f;(void)fn;(void)n;(void)m;(void)u;}
static double now(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec+t.tv_nsec/1e9;}
static AAF_Iface *load(const char *path){AAF_Iface *a=aafi_alloc(NULL);if(!a){exit(2);}aafi_set_debug(a,VERB_QUIET,0,NULL,quiet,NULL);if(aafi_load_file(a,path)){exit(3);}return a;}
int main(int argc,char **argv){if(argc!=5){return 1;}int iterations=atoi(argv[3]);if(iterations<1){return 1;}double start;
 if(!strcmp(argv[1],"parse")){start=now();for(int i=0;i<iterations;i++){AAF_Iface *a=load(argv[2]);aafi_release(&a);}}
 else if(!strcmp(argv[1],"extract")){AAF_Iface *a=load(argv[2]);start=now();for(int i=0;i<iterations;i++){aafiAudioEssenceFile *e=NULL;AAFI_foreachAudioEssenceFile(a,e){if(e->is_embedded&&aafi_extractAudioEssenceFile(a,e,AAFI_EXTRACT_WAV,argv[4],0,0,NULL,NULL)){return 4;}}}double elapsed=now()-start;aafi_release(&a);printf("%.9f\n",elapsed/iterations);return 0;}
 else if(!strcmp(argv[1],"profile")){AAF_Iface *a=load(argv[2]);fprintf(stderr,"profile PID %d\n",getpid());sleep(iterations);aafi_release(&a);return 0;}
 else{return 1;}printf("%.9f\n",(now()-start)/iterations);return 0;}
