# Scaletempo2 - Standalone Audio Time-Stretching Library

This is a standalone implementation of the scaletempo2 algorithm, extracted from the mpv media player. The algorithm uses Waveform Similarity Overlap-and-add (WSOLA) for high-quality audio time-stretching and pitch-shifting.

## Features

- High-quality time-stretching without pitch changes  
- Support for variable playback rates (0.25x to 4x by default)
- Multi-channel audio support (up to 8 channels)
- Low-latency processing suitable for real-time applications
- SIMD optimizations for improved performance (when available)
- Clean C API with no external dependencies except standard library

## Building

### Prerequisites
- GCC or compatible C compiler
- Make
- Standard C library with math support

### Build Commands
```bash
# Build static library (default)
make

# Build shared library  
make shared

# Build both static and shared
make static shared

# Build with debug symbols
make debug

# Build optimized version
make optimized

# Run tests
make test

# Clean build artifacts
make clean
```

## API Reference

### Core Functions

```c
// Create a new scaletempo2 instance
struct mp_scaletempo2* mp_scaletempo2_create(
    const struct mp_scaletempo2_opts* opts, 
    int channels, 
    int sample_rate
);

// Destroy instance and free memory
void mp_scaletempo2_destroy(struct mp_scaletempo2 *p);

// Reset internal state (useful when seeking)
void mp_scaletempo2_reset(struct mp_scaletempo2 *p);

// Feed input audio data
int mp_scaletempo2_fill_input_buffer(
    struct mp_scaletempo2 *p,
    uint8_t **planes,           // Array of channel data pointers (float*)
    int frame_size,             // Number of frames to process
    double playback_rate        // Playback speed multiplier
);

// Get processed output audio
int mp_scaletempo2_fill_buffer(
    struct mp_scaletempo2 *p,
    float **dest,               // Output buffer (array of channel pointers)
    int dest_size,              // Maximum frames to output
    double playback_rate        // Playback speed multiplier
);

// Mark end of input stream
void mp_scaletempo2_set_final(struct mp_scaletempo2 *p);

// Check if output frames are available
bool mp_scaletempo2_frames_available(struct mp_scaletempo2 *p, double playback_rate);

// Get current processing latency
double mp_scaletempo2_get_latency(struct mp_scaletempo2 *p, double playback_rate);
```

### Configuration

```c
// Get default configuration
struct mp_scaletempo2_opts mp_scaletempo2_get_default_opts(void);

// Configuration structure
struct mp_scaletempo2_opts {
    float min_playback_rate;        // Minimum supported rate (default: 0.25)
    float max_playback_rate;        // Maximum supported rate (default: 4.0) 
    float ola_window_size_ms;       // Overlap-add window size (default: 20ms)
    float wsola_search_interval_ms; // Search interval size (default: 30ms)
};
```

## Usage Example

```c
#include "scaletempo2.h"

int main() {
    // Create instance with default settings
    struct mp_scaletempo2_opts opts = mp_scaletempo2_get_default_opts();
    struct mp_scaletempo2* st2 = mp_scaletempo2_create(&opts, 2, 44100);
    
    // Prepare input data (planar format)
    float* input_channels[2];  // Stereo
    float* output_channels[2];
    
    // ... allocate and fill input_channels with audio data ...
    
    // Process at 1.5x speed
    double rate = 1.5;
    mp_scaletempo2_fill_input_buffer(st2, (uint8_t**)input_channels, 1024, rate);
    
    if (mp_scaletempo2_frames_available(st2, rate)) {
        int output_frames = mp_scaletempo2_fill_buffer(st2, output_channels, 1024, rate);
        // Use output_frames samples from output_channels
    }
    
    mp_scaletempo2_destroy(st2);
    return 0;
}
```

## Audio Format Requirements

- **Sample Format**: 32-bit floating point
- **Layout**: Planar (separate arrays per channel)
- **Channels**: 1-8 channels supported  
- **Sample Rate**: Any reasonable rate (tested with 8kHz-192kHz)

## Performance Notes

- The algorithm requires buffering input audio before producing output
- Larger buffer sizes generally provide better quality
- Processing latency depends on window size and search interval settings
- SIMD optimizations are automatically used when available (x86_64 with AVX)

## Algorithm Details

The scaletempo2 algorithm is based on Waveform Similarity Overlap-and-Add (WSOLA):

1. **Target Block**: Extract the "natural" continuation of the output
2. **Search Block**: Extract a larger block from input around the expected position  
3. **Optimal Block**: Find the most similar block to target within search block
4. **Transition**: Blend target and optimal blocks for smooth transitions
5. **Overlap-Add**: Combine with previous output using overlap-add windowing

This approach preserves audio quality better than simple resampling while maintaining relatively low computational cost.

## License

This code is based on Chromium's audio renderer algorithm and retains the original BSD license. See the source files for full license text.

## Troubleshooting

### No output frames produced
- Ensure you're feeding enough input data (typically 2-4 window sizes worth)
- Check that playback_rate is within the configured min/max range
- Verify input format is correct (32-bit float, planar)

### Quality issues  
- Try adjusting `ola_window_size_ms` (larger = better quality, more latency)
- Adjust `wsola_search_interval_ms` (larger = better quality, more CPU)
- Ensure consistent playback_rate (frequent changes can cause artifacts)

### Build issues
- Ensure you have a C11-compatible compiler
- Check that math library is linked (-lm)
- Try building without optimizations for debugging

## Files

- `scaletempo2.h` - Public API header
- `scaletempo2_internal.h` - Internal implementation header  
- `scaletempo2_internal.c` - Core algorithm implementation
- `scaletempo2_wrapper.c` - Public API wrapper
- `Makefile` - Build system
- `test_scaletempo2.c` - Basic functionality test
- `test_extended.c` - More comprehensive test