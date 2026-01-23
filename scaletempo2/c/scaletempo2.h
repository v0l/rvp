#pragma once

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

// Forward declaration
struct mp_scaletempo2;
struct mp_scaletempo2_opts;

/**
 * Default options for scaletempo2 algorithm
 */
struct mp_scaletempo2_opts {
    // Max/min supported playback rates for fast/slow audio. Audio outside of these
    // ranges are muted.
    float min_playback_rate;
    float max_playback_rate;
    // Overlap-and-add window size in milliseconds.
    float ola_window_size_ms;
    // Size of search interval in milliseconds. The search interval is
    // [-delta delta] around |output_index| * |playback_rate|. So the search
    // interval is 2 * delta.
    float wsola_search_interval_ms;
};

/**
 * Get default options for scaletempo2
 */
struct mp_scaletempo2_opts mp_scaletempo2_get_default_opts(void);

/**
 * Create a new scaletempo2 instance
 * @param opts Configuration options (pass NULL for defaults)
 * @param channels Number of audio channels
 * @param sample_rate Sample rate in Hz
 * @return New instance or NULL on error
 */
struct mp_scaletempo2* mp_scaletempo2_create(const struct mp_scaletempo2_opts* opts, 
                                           int channels, int sample_rate);

/**
 * Destroy scaletempo2 instance
 * @param p Instance to destroy
 */
void mp_scaletempo2_destroy(struct mp_scaletempo2 *p);

/**
 * Reset the internal state
 * @param p scaletempo2 instance
 */
void mp_scaletempo2_reset(struct mp_scaletempo2 *p);

/**
 * Fill input buffer with audio data
 * @param p scaletempo2 instance
 * @param planes Array of pointers to channel data (float samples)
 * @param frame_size Number of frames to add
 * @param playback_rate Current playback rate
 * @return Number of frames actually consumed
 */
int mp_scaletempo2_fill_input_buffer(struct mp_scaletempo2 *p,
    uint8_t **planes, int frame_size, double playback_rate);

/**
 * Process audio and fill output buffer
 * @param p scaletempo2 instance
 * @param dest Output buffer (array of pointers to channel data)
 * @param dest_size Maximum number of frames to output
 * @param playback_rate Current playback rate
 * @return Number of frames actually output
 */
int mp_scaletempo2_fill_buffer(struct mp_scaletempo2 *p,
    float **dest, int dest_size, double playback_rate);

/**
 * Mark input as final (end of stream)
 * @param p scaletempo2 instance
 */
void mp_scaletempo2_set_final(struct mp_scaletempo2 *p);

/**
 * Check if frames are available for output
 * @param p scaletempo2 instance
 * @param playback_rate Current playback rate
 * @return true if frames are available
 */
bool mp_scaletempo2_frames_available(struct mp_scaletempo2 *p, double playback_rate);

/**
 * Get current latency in frames
 * @param p scaletempo2 instance  
 * @param playback_rate Current playback rate
 * @return Latency in frames
 */
double mp_scaletempo2_get_latency(struct mp_scaletempo2 *p, double playback_rate);

#ifdef __cplusplus
}
#endif