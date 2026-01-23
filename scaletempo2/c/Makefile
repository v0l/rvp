# Makefile for standalone scaletempo2 audio time-stretching library

CC = gcc
AR = ar
CFLAGS = -Wall -O2 -fPIC -std=c11
INCLUDES = -I.

# Output directory
OUTDIR = out

# Core scaletempo2 source files
SOURCES = \
	scaletempo2_internal.c \
	scaletempo2_wrapper.c

# Object files (in output directory)
OBJECTS = $(addprefix $(OUTDIR)/, $(SOURCES:.c=.o))

# Output targets (in output directory)
TARGET_LIB = $(OUTDIR)/libscaletempo2.a
TARGET_SO = $(OUTDIR)/libscaletempo2.so

# Libraries
LDFLAGS = -lm

.PHONY: all clean static shared test

all: static

static: $(TARGET_LIB)

shared: $(TARGET_SO)

test: $(OUTDIR)/test_scaletempo2

# Create output directory
$(OUTDIR):
	@mkdir -p $(OUTDIR)

# Build static library
$(TARGET_LIB): $(OBJECTS) | $(OUTDIR)
	$(AR) rcs $@ $^
	@echo "Built static library: $@"

# Build shared library
$(TARGET_SO): $(OBJECTS) | $(OUTDIR)
	$(CC) -shared -o $@ $^ $(LDFLAGS)
	@echo "Built shared library: $@"

# Compile source files
$(OUTDIR)/%.o: %.c | $(OUTDIR)
	$(CC) $(CFLAGS) $(INCLUDES) -c $< -o $@

# Test executable
$(OUTDIR)/test_scaletempo2: test_scaletempo2.c $(TARGET_LIB) | $(OUTDIR)
	$(CC) $(CFLAGS) $(INCLUDES) -o $@ $< -L$(OUTDIR) -lscaletempo2 $(LDFLAGS)

clean:
	rm -rf $(OUTDIR)
	@echo "Cleaned build artifacts"

# Debug build with symbols
debug: CFLAGS += -g -DDEBUG
debug: clean static
	@echo "Built debug version"

# Optimized build
optimized: CFLAGS += -O3 -march=native
optimized: clean static
	@echo "Built optimized version"

info:
	@echo "CC: $(CC)"
	@echo "CFLAGS: $(CFLAGS)"
	@echo "Sources: $(SOURCES)"
	@echo "Objects: $(OBJECTS)"
	@echo "Target static: $(TARGET_LIB)"
	@echo "Target shared: $(TARGET_SO)"