// Sustained GPU load for thermal testing on the 83SC.
//
// vkcube only reached ~11W of the RTX 5050's 65W budget, which is not a
// thermal test. This saturates the SMs with dense FMA work and holds it, so
// the single shared heatpipe actually sees a realistic GPU heat load.
//
// build:  /opt/cuda/bin/nvcc -O3 -o gpuburn gpuburn.cu
// run:    ./gpuburn [seconds]

#include <cstdio>
#include <cstdlib>
#include <ctime>
#include <cuda_runtime.h>

// Long dependent FMA chain: no memory bottleneck, so the SMs stay busy and
// the power draw stays high and steady rather than bursty.
__global__ void burn(float *out, int iters)
{
    float a = threadIdx.x * 0.001f + 1.0f;
    float b = blockIdx.x * 0.001f + 1.0f;
    float c = 0.5f;
    for (int i = 0; i < iters; i++) {
        a = fmaf(a, b, c);
        b = fmaf(b, c, a);
        c = fmaf(c, a, b);
        // Keep values bounded so they never reach inf/NaN, which would let
        // the hardware short-circuit the arithmetic and drop power draw.
        a = a - floorf(a);
        b = b - floorf(b);
        c = c - floorf(c);
    }
    if (threadIdx.x == 1024)   // never true; stops the compiler eliding it
        out[blockIdx.x] = a + b + c;
}

int main(int argc, char **argv)
{
    int seconds = (argc > 1) ? atoi(argv[1]) : 300;

    cudaDeviceProp prop;
    if (cudaGetDeviceProperties(&prop, 0) != cudaSuccess) {
        fprintf(stderr, "no CUDA device (is the dGPU visible to this process?)\n");
        return 1;
    }
    printf("burning %s for %ds  (%d SMs)\n", prop.name, seconds, prop.multiProcessorCount);
    fflush(stdout);

    float *d_out = nullptr;
    if (cudaMalloc(&d_out, 4096 * sizeof(float)) != cudaSuccess) {
        fprintf(stderr, "cudaMalloc failed\n");
        return 1;
    }

    int blocks = prop.multiProcessorCount * 8;
    time_t end = time(nullptr) + seconds;
    while (time(nullptr) < end) {
        burn<<<blocks, 256>>>(d_out, 20000);
        cudaError_t err = cudaDeviceSynchronize();
        if (err != cudaSuccess) {
            fprintf(stderr, "kernel error: %s\n", cudaGetErrorString(err));
            break;
        }
    }
    cudaFree(d_out);
    printf("done\n");
    return 0;
}
