/* Development-only GPL-2.0-or-later direct extraction oracle. */
#include <libaaf/AAFIface.h>
#include <libaaf/AAFIEssenceFile.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static void quiet(struct aafLog *l,void *c,int b,int t,const char *f,const char *fn,int n,const char *m,void *u){(void)l;(void)c;(void)b;(void)t;(void)f;(void)fn;(void)n;(void)m;(void)u;}
int main(int argc,char **argv) {
    if(argc != 6) return 1;
    AAF_Iface *a=aafi_alloc(NULL);
    if(!a) return 2;
    aafi_set_debug(a,VERB_QUIET,0,NULL,quiet,NULL);
    if(aafi_load_file(a,argv[1])) return 3;
    enum aafiExtractFormat format = !strcmp(argv[2],"wav") ? AAFI_EXTRACT_WAV : !strcmp(argv[2],"bwav") ? AAFI_EXTRACT_BWAV : AAFI_EXTRACT_DEFAULT;
    unsigned long long offset=strtoull(argv[4],NULL,10), length=strtoull(argv[5],NULL,10);
    aafiAudioEssenceFile *e=NULL;
    int index=0;
    AAFI_foreachAudioEssenceFile(a,e) {
        if(!e->is_embedded) continue;
        char name[32]; snprintf(name,sizeof(name),"essence-%d",index);
        int rc=aafi_extractAudioEssenceFile(a,e,format,argv[3],offset,length,name,NULL);
        printf("%d:%s\n",index++,rc ? "error" : "ok");
    }
    aafi_release(&a);
    return 0;
}
