#include <string.h>
#include "scaletempo2.h"
#include "scaletempo2_internal.h"

struct mp_scaletempo2_opts mp_scaletempo2_get_default_opts(void)
{
    struct mp_scaletempo2_opts opts = {
        .min_playback_rate = 0.25f,
        .max_playback_rate = 4.0f,
        .ola_window_size_ms = 20.0f,
        .wsola_search_interval_ms = 30.0f
    };
    return opts;
}

struct mp_scaletempo2* mp_scaletempo2_create(const struct mp_scaletempo2_opts* opts, 
                                           int channels, int sample_rate)
{
    if (channels <= 0 || channels > MP_NUM_CHANNELS || sample_rate <= 0) {
        return NULL;
    }

    struct mp_scaletempo2* p = calloc(1, sizeof(struct mp_scaletempo2));
    if (!p) {
        return NULL;
    }

    if (opts) {
        p->opts = malloc(sizeof(struct mp_scaletempo2_opts));
        if (!p->opts) {
            free(p);
            return NULL;
        }
        *p->opts = *opts;
    } else {
        p->opts = malloc(sizeof(struct mp_scaletempo2_opts));
        if (!p->opts) {
            free(p);
            return NULL;
        }
        *p->opts = mp_scaletempo2_get_default_opts();
    }

    mp_scaletempo2_init(p, channels, sample_rate);
    return p;
}

void mp_scaletempo2_destroy(struct mp_scaletempo2 *p)
{
    if (!p) return;
    
    // Free all allocated buffers
    if (p->wsola_output) {
        for (int i = 0; i < p->channels; ++i) {
            free(p->wsola_output[i]);
        }
        free(p->wsola_output);
    }
    
    if (p->optimal_block) {
        for (int i = 0; i < p->channels; ++i) {
            free(p->optimal_block[i]);
        }
        free(p->optimal_block);
    }
    
    if (p->search_block) {
        for (int i = 0; i < p->channels; ++i) {
            free(p->search_block[i]);
        }
        free(p->search_block);
    }
    
    if (p->target_block) {
        for (int i = 0; i < p->channels; ++i) {
            free(p->target_block[i]);
        }
        free(p->target_block);
    }
    
    if (p->input_buffer) {
        for (int i = 0; i < p->channels; ++i) {
            free(p->input_buffer[i]);
        }
        free(p->input_buffer);
    }
    
    free(p->ola_window);
    free(p->transition_window);
    free(p->energy_candidate_blocks);
    free(p->opts);
    
    // Clear the structure
    memset(p, 0, sizeof(*p));
    free(p);
}