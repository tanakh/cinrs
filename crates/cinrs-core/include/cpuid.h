/* <cpuid.h> — GCC's CPUID helpers, in C on cinrs's inline assembly.
 *
 * The same names and meanings as GCC's header: the __cpuid and __cpuid_count
 * macros store the four registers into four lvalues, __get_cpuid_max returns
 * the highest leaf of a range, __get_cpuid and __get_cpuid_count return 0 for
 * a leaf the processor does not have, and the bit_* and signature_* macros
 * decode the answer.
 *
 * GCC names rbx as an operand ("=b"); rustc keeps rbx for LLVM and refuses it
 * as an asm! operand or clobber. So each macro saves and restores it in the
 * template instead — `xchgq %rbx, %q1; cpuid; xchgq %rbx, %q1` with an
 * early-clobbered register for operand 1 — which is the idiom GCC's own header
 * uses for 32-bit PIC code. The processor sees the same instruction.
 *
 * Two differences from GCC, both narrowing what GCC leaves undefined:
 * __get_cpuid passes subleaf 0 in ecx (GCC leaves ecx as it happens to be,
 * which matters for leaf 7 and the other subleaf-indexed leaves), and
 * __get_cpuid_max on 32-bit x86 does not test the EFLAGS ID bit for a
 * processor older than the Pentium.
 */
#ifndef _CINRS_CPUID_H
#define _CINRS_CPUID_H

#if !defined(__i386__) && !defined(__x86_64__)
#error "<cpuid.h> is x86 only: CPUID is an x86 instruction, and this unit is being translated for another architecture. Guard the #include with #if defined(__x86_64__) || defined(__i386__)."
#else

/* Leaf 1, %ecx. */
#define bit_SSE3        (1 << 0)
#define bit_PCLMUL      (1 << 1)
#define bit_SSSE3       (1 << 9)
#define bit_FMA         (1 << 12)
#define bit_CMPXCHG16B  (1 << 13)
#define bit_SSE4_1      (1 << 19)
#define bit_SSE4_2      (1 << 20)
#define bit_MOVBE       (1 << 22)
#define bit_POPCNT      (1 << 23)
#define bit_AES         (1 << 25)
#define bit_XSAVE       (1 << 26)
#define bit_OSXSAVE     (1 << 27)
#define bit_AVX         (1 << 28)
#define bit_F16C        (1 << 29)
#define bit_RDRND       (1 << 30)

/* Leaf 1, %edx. */
#define bit_CMPXCHG8B   (1 << 8)
#define bit_CMOV        (1 << 15)
#define bit_MMX         (1 << 23)
#define bit_FXSAVE      (1 << 24)
#define bit_SSE         (1 << 25)
#define bit_SSE2        (1 << 26)

/* Leaf 0x80000001, %ecx. */
#define bit_LAHF_LM     (1 << 0)
#define bit_ABM         (1 << 5)
#define bit_LZCNT       bit_ABM
#define bit_SSE4a       (1 << 6)
#define bit_PRFCHW      (1 << 8)
#define bit_XOP         (1 << 11)
#define bit_LWP         (1 << 15)
#define bit_FMA4        (1 << 16)
#define bit_TBM         (1 << 21)
#define bit_MWAITX      (1 << 29)

/* Leaf 0x80000001, %edx. */
#define bit_MMXEXT      (1 << 22)
#define bit_LM          (1 << 29)
#define bit_3DNOWP      (1 << 30)
#define bit_3DNOW       (1u << 31)

/* Leaf 7, subleaf 0, %ebx. */
#define bit_FSGSBASE    (1 << 0)
#define bit_SGX         (1 << 2)
#define bit_BMI         (1 << 3)
#define bit_HLE         (1 << 4)
#define bit_AVX2        (1 << 5)
#define bit_SMEP        (1 << 7)
#define bit_BMI2        (1 << 8)
#define bit_RTM         (1 << 11)
#define bit_AVX512F     (1 << 16)
#define bit_RDSEED      (1 << 18)
#define bit_ADX         (1 << 19)
#define bit_CLFLUSHOPT  (1 << 23)
#define bit_CLWB        (1 << 24)
#define bit_SHA         (1 << 29)

/* Leaf 7, subleaf 0, %ecx. */
#define bit_PREFETCHWT1 (1 << 0)
#define bit_AVX512VBMI  (1 << 1)
#define bit_PKU         (1 << 3)
#define bit_OSPKE       (1 << 4)
#define bit_VAES        (1 << 9)
#define bit_VPCLMULQDQ  (1 << 10)
#define bit_RDPID       (1 << 22)

/* The vendor string leaf 0 returns in %ebx, %edx, %ecx. */
#define signature_INTEL_ebx   0x756e6547
#define signature_INTEL_edx   0x49656e69
#define signature_INTEL_ecx   0x6c65746e
#define signature_AMD_ebx     0x68747541
#define signature_AMD_edx     0x69746e65
#define signature_AMD_ecx     0x444d4163
#define signature_CENTAUR_ebx 0x746e6543
#define signature_CENTAUR_edx 0x48727561
#define signature_CENTAUR_ecx 0x736c7561
#define signature_HYGON_ebx   0x6f677948
#define signature_HYGON_edx   0x6e65476e
#define signature_HYGON_ecx   0x656e6975

#ifdef __x86_64__
#define __CINRS_CPUID_INSN \
  "xchgq %%rbx, %q1\n\tcpuid\n\txchgq %%rbx, %q1"
#else
#define __CINRS_CPUID_INSN \
  "xchgl %%ebx, %k1\n\tcpuid\n\txchgl %%ebx, %k1"
#endif

#define __cpuid(__level, __a, __b, __c, __d) \
  __asm__ __volatile__ (__CINRS_CPUID_INSN \
                        : "=a" (__a), "=&r" (__b), "=c" (__c), "=d" (__d) \
                        : "0" (__level))

#define __cpuid_count(__level, __count, __a, __b, __c, __d) \
  __asm__ __volatile__ (__CINRS_CPUID_INSN \
                        : "=a" (__a), "=&r" (__b), "=c" (__c), "=d" (__d) \
                        : "0" (__level), "2" (__count))

/* The highest leaf of the range __ext starts (0 or 0x80000000), and the
 * vendor's %ebx in *__sig when __sig is not null. */
static __inline unsigned int
__get_cpuid_max (unsigned int __ext, unsigned int *__sig)
{
  unsigned int __eax, __ebx, __ecx, __edx;
  __cpuid_count (__ext, 0, __eax, __ebx, __ecx, __edx);
  if (__sig)
    *__sig = __ebx;
  return __eax;
}

/* Leaf __leaf, subleaf __subleaf, into the four pointers; 0 when the
 * processor does not have the leaf. */
static __inline int
__get_cpuid_count (unsigned int __leaf, unsigned int __subleaf,
                   unsigned int *__eax, unsigned int *__ebx,
                   unsigned int *__ecx, unsigned int *__edx)
{
  unsigned int __ext = __leaf & 0x80000000;
  unsigned int __maxlevel = __get_cpuid_max (__ext, 0);
  if (__maxlevel == 0 || __maxlevel < __leaf)
    return 0;
  __cpuid_count (__leaf, __subleaf, *__eax, *__ebx, *__ecx, *__edx);
  return 1;
}

/* Leaf __leaf (subleaf 0) into the four pointers; 0 when the processor does
 * not have the leaf. */
static __inline int
__get_cpuid (unsigned int __leaf, unsigned int *__eax, unsigned int *__ebx,
             unsigned int *__ecx, unsigned int *__edx)
{
  return __get_cpuid_count (__leaf, 0, __eax, __ebx, __ecx, __edx);
}

#endif /* x86 */
#endif /* _CINRS_CPUID_H */
