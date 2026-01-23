#include <stdio.h>
#include <stdlib.h>
#include <math.h>
#include "scaletempo2.h"

#ifndef M_PI
#define M_PI 3.14159265358979323846
#endif

// Generate a simple sine wave for testing
void generate_sine_wave(float *buffer, int num_samples, int channels, 
                       float frequency, float sample_rate, float *phase) 
{
    for (int i = 0; i < num_samples; i++) {
        float sample = sin(2.0 * M_PI * frequency * (*phase) / sample_rate);
        for (int ch = 0; ch < channels; ch++) {
            buffer[i * channels + ch] = sample * 0.5f;
        }
        (*phase) += 1.0f;
        if (*phase >= sample_rate) *phase = 0.0f;
    }
}

int main()
{
    printf("Testing scaletempo2 library...\n");

    const int channels = 2;
    const int sample_rate = 44100;
    const int buffer_size = 3000;
    
    struct mp_scaletempo2_opts opts = mp_scaletempo2_get_default_opts();
    printf("Options: min=%.2f, max=%.2f, window=%.1fms, search=%.1fms\n",
           opts.min_playback_rate, opts.max_playback_rate, 
           opts.ola_window_size_ms, opts.wsola_search_interval_ms);

    struct mp_scaletempo2* st2 = mp_scaletempo2_create(&opts, channels, sample_rate);
    if (!st2) {
        printf("Failed to create scaletempo2 instance\n");
        return 1;
    }

    printf("Created scaletempo2 instance successfully\n");

    // Test key playback rates
    double test_rates[] = {0.5, 1.0, 1.5, 2.0};
    int num_rates = sizeof(test_rates) / sizeof(test_rates[0]);

    for (int r = 0; r < num_rates; r++) {
        double rate = test_rates[r];
        printf("\nTesting playback rate: %.1fx\n", rate);
        
        mp_scaletempo2_reset(st2);
        
        // Generate test input
        float input_data[buffer_size * channels];
        float phase = 0.0f;
        generate_sine_wave(input_data, buffer_size, channels, 440.0f, sample_rate, &phase);
        
        // Convert to planar format
        float *input_planes[channels];
        for (int ch = 0; ch < channels; ch++) {
            input_planes[ch] = malloc(buffer_size * sizeof(float));
            for (int i = 0; i < buffer_size; i++) {
                input_planes[ch][i] = input_data[i * channels + ch];
            }
        }
        
        // Process audio
        int consumed = mp_scaletempo2_fill_input_buffer(st2, (uint8_t**)input_planes, buffer_size, rate);
        
        float *output_planes[channels];
        for (int ch = 0; ch < channels; ch++) {
            output_planes[ch] = malloc(buffer_size * sizeof(float));
        }
        
        int total_output = 0;
        if (mp_scaletempo2_frames_available(st2, rate)) {
            int output_frames = mp_scaletempo2_fill_buffer(st2, output_planes, buffer_size, rate);
            total_output += output_frames;
            
            mp_scaletempo2_set_final(st2);
            while (mp_scaletempo2_frames_available(st2, rate)) {
                output_frames = mp_scaletempo2_fill_buffer(st2, output_planes, buffer_size, rate);
                if (output_frames <= 0) break;
                total_output += output_frames;
            }
        }
        
        printf("  Input: %d frames, Output: %d frames", consumed, total_output);
        if (total_output > 0) {
            printf(" (ratio: %.2f)", (double)total_output / consumed);
        }
        printf("\n");
        
        // Cleanup
        for (int ch = 0; ch < channels; ch++) {
            free(input_planes[ch]);
            free(output_planes[ch]);
        }
    }

    mp_scaletempo2_destroy(st2);
    printf("\nTest completed successfully!\n");
    return 0;
}