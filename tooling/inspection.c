/* GPL-2.0-or-later. Development oracle: original bytes and stable CFB node IDs.
 * C's normal parsed-property dump prints pointer-bearing object memory. */
#include <libaaf/AAFIface.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
static void quiet(struct aafLog*l,void*c,int b,int t,const char*f,const char*fn,int n,const char*m,void*u){(void)l;(void)c;(void)b;(void)t;(void)f;(void)fn;(void)n;(void)m;(void)u;}
static unsigned u16(const unsigned char*b){return b[0]|((unsigned)b[1]<<8);}
static void hex(const void*p,size_t n){const unsigned char*b=p;for(size_t i=0;i<n;i++)printf("%02x",b[i]);}
int main(int argc,char**argv){
 if(argc!=2)return 1;
 AAF_Iface*a=aafi_alloc(NULL);if(!a)return 2;
 aafi_set_debug(a,VERB_QUIET,0,NULL,quiet,NULL);
 if(aafi_load_file(a,argv[1]))return 3;
 for(aafObject*o=a->aafd->Objects;o;o=o->nextObj){
  unsigned id=(unsigned)(o->Node-a->aafd->cfbd->nodes);
  printf("O\t%u\t",id);hex(o->Class->ID,16);putchar('\n');
  cfbNode*node=cfb_getChildNode(a->aafd->cfbd,"properties",o->Node);
  unsigned char*b=NULL;uint64_t n=0;
  if(!node||!cfb_getStream(a->aafd->cfbd,node,&b,&n)||n<4)return 4;
  unsigned count=u16(b+2);size_t at=4+(size_t)count*6;if(at>n)return 5;
  for(unsigned i=0;i<count;i++){
   unsigned pid=u16(b+4+i*6),sf=u16(b+6+i*6),len=u16(b+8+i*6);
   if(len>n-at)return 6;
   printf("P\t%u\t%04x\t%04x\t",id,pid,sf);hex(b+at,len);putchar('\t');
   if(sf==0x22||sf==0x32||sf==0x3a){
    aafProperty*p=o->Properties;for(;p&&p->pid!=pid;p=p->next){}
    if(p){aafObject*target=p->val;int first=1;while(target){if(!first)putchar(',');printf("%u",(unsigned)(target->Node-a->aafd->cfbd->nodes));first=0;if(sf==0x22)break;target=target->next;}}
   }
   putchar('\n');at+=len;
  }
  free(b);
 }
 aafi_release(&a);return 0;
}
