#include "hvfs.h"
#include "ata.h"
#include "serial.h"
#include "kernel.h"
#include "pwid.h"
#include "string.h"
#include "assert.h"
#include "kmalloc.h"

struct super_block hvfs_super;

static struct inode *hvfs_inode_table = NULL;
static uint8_t *hvfs_block_bitmap = NULL;
static uint8_t *hvfs_inode_bitmap = NULL;
static uint32_t hvfs_inode_table_size = 0;
static uint32_t hvfs_block_bitmap_size = 0;
static uint32_t hvfs_inode_bitmap_size = 0;

static struct block_cache_entry *block_cache = NULL;
static uint32_t cache_access_counter = 0;

static struct hvfs_context current_context;
static int hvfs_initialized = 0;
static int hvfs_disk_mode = 0;
static uint32_t next_fd = 3;

static struct inode* resolve_path(const char *path, uint64_t pwid);

static uint64_t get_time(void) {
    uint64_t tsc;
    __asm__ volatile ("rdtsc" : "=A"(tsc));
    return tsc;
}

static uint8_t* get_block(uint32_t block_num) {
    if (block_num >= hvfs_super.total_blocks) return NULL;
    if (block_cache == NULL) return NULL;
    
    for (int i = 0; i < HVFS_CACHE_SIZE; i++) {
        if (block_cache[i].valid && block_cache[i].block_num == block_num) {
            block_cache[i].access_time = ++cache_access_counter;
            return block_cache[i].data;
        }
    }
    
    int lru_idx = -1;
    
    for (int i = 0; i < HVFS_CACHE_SIZE; i++) {
        if (!block_cache[i].valid) {
            lru_idx = i;
            break;
        }
    }
    
    if (lru_idx == -1) {
        lru_idx = 0;
        uint32_t lru_time = block_cache[0].access_time;
        
        for (int i = 1; i < HVFS_CACHE_SIZE; i++) {
            if (block_cache[i].access_time < lru_time) {
                lru_time = block_cache[i].access_time;
                lru_idx = i;
            }
        }
    }
    
    if (block_cache[lru_idx].data == NULL) {
        block_cache[lru_idx].data = (uint8_t*)kmalloc(HVFS_BLOCK_SIZE);
        if (block_cache[lru_idx].data == NULL) return NULL;
    }
    
    if (block_cache[lru_idx].valid && block_cache[lru_idx].dirty) {
        if (hvfs_disk_mode) {
            for (int s = 0; s < HVFS_DISK_SECTORS_PER_BLOCK; s++) {
                ata_write_sector(0, HVFS_DATA_SECTOR_START + block_cache[lru_idx].block_num * HVFS_DISK_SECTORS_PER_BLOCK + s,
                    block_cache[lru_idx].data + s * HVFS_DISK_SECTOR_SIZE);
            }
        }
    }
    
    block_cache[lru_idx].block_num = block_num;
    block_cache[lru_idx].valid = 1;
    block_cache[lru_idx].dirty = 0;
    block_cache[lru_idx].access_time = ++cache_access_counter;
    
    if (hvfs_disk_mode) {
        for (int s = 0; s < HVFS_DISK_SECTORS_PER_BLOCK; s++) {
            ata_read_sector(0, HVFS_DATA_SECTOR_START + block_num * HVFS_DISK_SECTORS_PER_BLOCK + s,
                block_cache[lru_idx].data + s * HVFS_DISK_SECTOR_SIZE);
        }
    } else {
        extern void *memset_optimized(void *s, int c, size_t n);
        memset_optimized(block_cache[lru_idx].data, 0, HVFS_BLOCK_SIZE);
    }
    
    return block_cache[lru_idx].data;
}

static void mark_block_dirty(uint32_t block_num) {
    for (int i = 0; i < HVFS_CACHE_SIZE; i++) {
        if (block_cache[i].valid && block_cache[i].block_num == block_num) {
            block_cache[i].dirty = 1;
            return;
        }
    }
}

static int block_is_free(uint32_t block_num) {
    if (block_num >= hvfs_super.total_blocks) return 0;
    uint32_t byte_idx = block_num / 8;
    uint32_t bit_idx = block_num % 8;
    if (byte_idx >= hvfs_block_bitmap_size) return 0;
    return !(hvfs_block_bitmap[byte_idx] & (1 << bit_idx));
}

static void block_set_used(uint32_t block_num) {
    if (block_num >= hvfs_super.total_blocks) return;
    uint32_t byte_idx = block_num / 8;
    uint32_t bit_idx = block_num % 8;
    if (byte_idx >= hvfs_block_bitmap_size) return;
    hvfs_block_bitmap[byte_idx] |= (1 << bit_idx);
    hvfs_super.free_blocks--;
}

static void block_set_free(uint32_t block_num) {
    if (block_num >= hvfs_super.total_blocks) return;
    uint32_t byte_idx = block_num / 8;
    uint32_t bit_idx = block_num % 8;
    if (byte_idx >= hvfs_block_bitmap_size) return;
    hvfs_block_bitmap[byte_idx] &= ~(1 << bit_idx);
    hvfs_super.free_blocks++;
}

#define INDIRECT_BLOCKS_PER_BLOCK (HVFS_BLOCK_SIZE / sizeof(uint32_t))

static uint32_t get_block_for_index(struct inode *inode, uint32_t block_idx, int alloc);
static int hvfs_load_data_block(uint32_t block_num);
static int hvfs_sync_data_block(uint32_t block_num);

static uint32_t block_alloc(void) {
    for (uint32_t i = hvfs_super.first_data_block; i < hvfs_super.total_blocks; i++) {
        if (block_is_free(i)) {
            block_set_used(i);
            uint8_t *block = get_block(i);
            if (block) {
                extern void *memset_optimized(void *s, int c, size_t n);
                memset_optimized(block, 0, HVFS_BLOCK_SIZE);
            }
            mark_block_dirty(i);
            return i;
        }
    }
    
    if (hvfs_super.total_blocks < HVFS_MAX_BLOCKS_LIMIT) {
        uint32_t new_total = hvfs_super.total_blocks * 2;
        if (new_total > HVFS_MAX_BLOCKS_LIMIT) new_total = HVFS_MAX_BLOCKS_LIMIT;
        
        if (hvfs_expand_blocks(new_total) == 0) {
            return block_alloc();
        }
    }
    
    return 0;
}

static void inode_set_used(uint32_t inode_num) {
    if (inode_num >= hvfs_super.inode_count) return;
    uint32_t byte_idx = inode_num / 8;
    uint32_t bit_idx = inode_num % 8;
    if (byte_idx >= hvfs_inode_bitmap_size) return;
    hvfs_inode_bitmap[byte_idx] |= (1 << bit_idx);
    hvfs_super.free_inodes--;
}

static void inode_set_free(uint32_t inode_num) {
    if (inode_num >= hvfs_super.inode_count) return;
    uint32_t byte_idx = inode_num / 8;
    uint32_t bit_idx = inode_num % 8;
    if (byte_idx >= hvfs_inode_bitmap_size) return;
    hvfs_inode_bitmap[byte_idx] &= ~(1 << bit_idx);
    hvfs_super.free_inodes++;
}

static struct inode* inode_alloc(void) {
    for (uint32_t i = 1; i < hvfs_super.inode_count; i++) {
        if (!hvfs_inode_table[i].used) {
            hvfs_inode_table[i].used = 1;
            hvfs_inode_table[i].inode_num = i;
            hvfs_inode_table[i].ref_count = 1;
            hvfs_inode_table[i].link_count = 1;
            hvfs_inode_table[i].dirty = 0;
            hvfs_inode_table[i].in_cache = 1;
            inode_set_used(i);
            return &hvfs_inode_table[i];
        }
    }
    
    if (hvfs_super.inode_count < HVFS_MAX_INODES_LIMIT) {
        uint32_t new_count = hvfs_super.inode_count * 2;
        if (new_count > HVFS_MAX_INODES_LIMIT) new_count = HVFS_MAX_INODES_LIMIT;
        
        if (hvfs_expand_inodes(new_count) == 0) {
            return inode_alloc();
        }
    }
    
    return NULL;
}

static void inode_free(struct inode *inode) {
    if (inode == NULL || inode->inode_num == 0) return;
    
    inode->ref_count--;
    if (inode->ref_count <= 0 && inode->link_count <= 0) {
        for (int i = 0; i < 12; i++) {
            if (inode->direct_blocks[i] != 0) {
                block_set_free(inode->direct_blocks[i]);
                inode->direct_blocks[i] = 0;
            }
        }
        
        if (inode->indirect_block != 0) {
            uint32_t *indirect = (uint32_t *)get_block(inode->indirect_block);
            if (indirect) {
                for (uint32_t i = 0; i < INDIRECT_BLOCKS_PER_BLOCK; i++) {
                    if (indirect[i] != 0) {
                        block_set_free(indirect[i]);
                    }
                }
            }
            block_set_free(inode->indirect_block);
            inode->indirect_block = 0;
        }
        
        if (inode->double_indirect != 0) {
            uint32_t *double_indirect = (uint32_t *)get_block(inode->double_indirect);
            if (double_indirect) {
                for (uint32_t i = 0; i < INDIRECT_BLOCKS_PER_BLOCK; i++) {
                    if (double_indirect[i] != 0) {
                        uint32_t *indirect = (uint32_t *)get_block(double_indirect[i]);
                        if (indirect) {
                            for (uint32_t j = 0; j < INDIRECT_BLOCKS_PER_BLOCK; j++) {
                                if (indirect[j] != 0) {
                                    block_set_free(indirect[j]);
                                }
                            }
                        }
                        block_set_free(double_indirect[i]);
                    }
                }
            }
            block_set_free(inode->double_indirect);
            inode->double_indirect = 0;
        }
        
        if (inode->triple_indirect != 0) {
            uint32_t *triple = (uint32_t *)get_block(inode->triple_indirect);
            if (triple) {
                for (uint32_t i = 0; i < INDIRECT_BLOCKS_PER_BLOCK; i++) {
                    if (triple[i] != 0) {
                        uint32_t *double_indirect = (uint32_t *)get_block(triple[i]);
                        if (double_indirect) {
                            for (uint32_t j = 0; j < INDIRECT_BLOCKS_PER_BLOCK; j++) {
                                if (double_indirect[j] != 0) {
                                    uint32_t *indirect = (uint32_t *)get_block(double_indirect[j]);
                                    if (indirect) {
                                        for (uint32_t k = 0; k < INDIRECT_BLOCKS_PER_BLOCK; k++) {
                                            if (indirect[k] != 0) {
                                                block_set_free(indirect[k]);
                                            }
                                        }
                                    }
                                    block_set_free(double_indirect[j]);
                                }
                            }
                        }
                        block_set_free(triple[i]);
                    }
                }
            }
            block_set_free(inode->triple_indirect);
            inode->triple_indirect = 0;
        }
        
        inode_set_free(inode->inode_num);
        inode->used = 0;
        inode->inode_num = 0;
    }
}

static uint32_t get_block_for_index(struct inode *inode, uint32_t block_idx, int alloc) {
    uint64_t max_blocks = 12 + INDIRECT_BLOCKS_PER_BLOCK + 
                          INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK +
                          INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK;
    
    if (inode == NULL || block_idx >= max_blocks) return 0;
    
    if (block_idx < 12) {
        return inode->direct_blocks[block_idx];
    }
    
    uint32_t indirect_start = 12;
    uint32_t indirect_end = 12 + INDIRECT_BLOCKS_PER_BLOCK;
    
    if (block_idx >= indirect_start && block_idx < indirect_end) {
        if (inode->indirect_block == 0) {
            if (!alloc) return 0;
            inode->indirect_block = block_alloc();
            if (inode->indirect_block == 0) return 0;
            inode->dirty = 1;
        }
        
        uint32_t *indirect = (uint32_t *)get_block(inode->indirect_block);
        if (!indirect) return 0;
        
        uint32_t idx = block_idx - indirect_start;
        
        if (indirect[idx] == 0 && alloc) {
            indirect[idx] = block_alloc();
            if (indirect[idx] != 0) {
                inode->dirty = 1;
                mark_block_dirty(inode->indirect_block);
            }
        }
        
        return indirect[idx];
    }
    
    uint32_t double_start = indirect_end;
    uint32_t double_end = indirect_end + INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK;
    
    if (block_idx >= double_start && block_idx < double_end) {
        if (inode->double_indirect == 0) {
            if (!alloc) return 0;
            inode->double_indirect = block_alloc();
            if (inode->double_indirect == 0) return 0;
            inode->dirty = 1;
        }
        
        uint32_t *double_indirect = (uint32_t *)get_block(inode->double_indirect);
        if (!double_indirect) return 0;
        
        uint32_t double_idx = block_idx - double_start;
        uint32_t first_level = double_idx / INDIRECT_BLOCKS_PER_BLOCK;
        uint32_t second_level = double_idx % INDIRECT_BLOCKS_PER_BLOCK;
        
        if (double_indirect[first_level] == 0 && alloc) {
            double_indirect[first_level] = block_alloc();
            if (double_indirect[first_level] != 0) {
                inode->dirty = 1;
                mark_block_dirty(inode->double_indirect);
            }
        }
        
        if (double_indirect[first_level] == 0) return 0;
        
        uint32_t *indirect = (uint32_t *)get_block(double_indirect[first_level]);
        if (!indirect) return 0;
        
        if (indirect[second_level] == 0 && alloc) {
            indirect[second_level] = block_alloc();
            if (indirect[second_level] != 0) {
                inode->dirty = 1;
                mark_block_dirty(double_indirect[first_level]);
            }
        }
        
        return indirect[second_level];
    }
    
    uint32_t triple_start = double_end;
    uint32_t triple_idx = block_idx - triple_start;
    uint32_t first_level = triple_idx / (INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK);
    uint32_t remainder = triple_idx % (INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK);
    uint32_t second_level = remainder / INDIRECT_BLOCKS_PER_BLOCK;
    uint32_t third_level = remainder % INDIRECT_BLOCKS_PER_BLOCK;
    
    if (inode->triple_indirect == 0) {
        if (!alloc) return 0;
        inode->triple_indirect = block_alloc();
        if (inode->triple_indirect == 0) return 0;
        inode->dirty = 1;
    }
    
    uint32_t *triple = (uint32_t *)get_block(inode->triple_indirect);
    if (!triple) return 0;
    
    if (triple[first_level] == 0 && alloc) {
        triple[first_level] = block_alloc();
        if (triple[first_level] != 0) {
            inode->dirty = 1;
            mark_block_dirty(inode->triple_indirect);
        }
    }
    
    if (triple[first_level] == 0) return 0;
    
    uint32_t *double_indirect = (uint32_t *)get_block(triple[first_level]);
    if (!double_indirect) return 0;
    
    if (double_indirect[second_level] == 0 && alloc) {
        double_indirect[second_level] = block_alloc();
        if (double_indirect[second_level] != 0) {
            inode->dirty = 1;
            mark_block_dirty(triple[first_level]);
        }
    }
    
    if (double_indirect[second_level] == 0) return 0;
    
    uint32_t *indirect = (uint32_t *)get_block(double_indirect[second_level]);
    if (!indirect) return 0;
    
    if (indirect[third_level] == 0 && alloc) {
        indirect[third_level] = block_alloc();
        if (indirect[third_level] != 0) {
            inode->dirty = 1;
            mark_block_dirty(double_indirect[second_level]);
        }
    }
    
    return indirect[third_level];
}

int hvfs_is_disk_mode(void) {
    return hvfs_disk_mode;
}

struct inode* hvfs_get_inode(uint32_t inode_num) {
    if (inode_num == 0 || inode_num >= hvfs_super.inode_count) return NULL;
    if (!hvfs_inode_table[inode_num].used) return NULL;
    return &hvfs_inode_table[inode_num];
}

struct inode* hvfs_find_inode(const char *path) {
    return resolve_path(path, hvfs_get_current_pwid());
}

int hvfs_check_permission(struct inode *inode, uint64_t pwid, int access_type) {
    if (inode == NULL) return 0;
    
    uint8_t level = pwid_get_level(pwid);
    
    if (level == PWID_LEVEL_ROOT) {
        return 1;
    }
    
    if (pwid == inode->owner_pwid) {
        uint16_t owner_perm = (inode->pwid_perm >> 8) & 0x0F;
        return (owner_perm & access_type) == access_type;
    }
    
    uint16_t other_perm = inode->pwid_perm & 0x0F;
    return (other_perm & access_type) == access_type;
}

int hvfs_expand_inodes(uint32_t new_count) {
    if (new_count <= hvfs_super.inode_count) return -1;
    if (new_count > HVFS_MAX_INODES_LIMIT) return -1;
    
    uint32_t new_bitmap_size = (new_count + 7) / 8;
    
    struct inode *new_table = (struct inode*)krealloc(hvfs_inode_table, new_count * sizeof(struct inode));
    if (new_table == NULL) {
        new_table = (struct inode*)kmalloc(new_count * sizeof(struct inode));
        if (new_table == NULL) {
            serial_puts(SERIAL_COM1, "HvFS: Failed to expand inode table\n");
            return -1;
        }
        if (hvfs_inode_table != NULL) {
            memcpy(new_table, hvfs_inode_table, hvfs_super.inode_count * sizeof(struct inode));
            kfree(hvfs_inode_table);
        }
    }
    
    uint8_t *new_bitmap = (uint8_t*)krealloc(hvfs_inode_bitmap, new_bitmap_size);
    if (new_bitmap == NULL) {
        new_bitmap = (uint8_t*)kmalloc(new_bitmap_size);
        if (new_bitmap == NULL) {
            serial_puts(SERIAL_COM1, "HvFS: Failed to expand inode bitmap\n");
            return -1;
        }
        if (hvfs_inode_bitmap != NULL) {
            memcpy(new_bitmap, hvfs_inode_bitmap, hvfs_inode_bitmap_size);
            kfree(hvfs_inode_bitmap);
        }
    }
    
    for (uint32_t i = hvfs_super.inode_count; i < new_count; i++) {
        memset(&new_table[i], 0, sizeof(struct inode));
    }
    
    for (uint32_t i = hvfs_inode_bitmap_size; i < new_bitmap_size; i++) {
        new_bitmap[i] = 0;
    }
    
    hvfs_inode_table = new_table;
    hvfs_inode_bitmap = new_bitmap;
    hvfs_super.free_inodes += (new_count - hvfs_super.inode_count);
    hvfs_super.inode_count = new_count;
    hvfs_inode_table_size = new_count;
    hvfs_inode_bitmap_size = new_bitmap_size;
    hvfs_super.dynamic_inodes = 1;
    
    serial_puts(SERIAL_COM1, "HvFS: Expanded inodes to ");
    serial_put_dec(SERIAL_COM1, new_count);
    serial_puts(SERIAL_COM1, "\n");
    
    return 0;
}

int hvfs_expand_blocks(uint32_t new_count) {
    if (new_count <= hvfs_super.total_blocks) return -1;
    if (new_count > HVFS_MAX_BLOCKS_LIMIT) return -1;
    
    uint32_t new_bitmap_size = (new_count + 7) / 8;
    
    uint8_t *new_bitmap = (uint8_t*)krealloc(hvfs_block_bitmap, new_bitmap_size);
    if (new_bitmap == NULL) {
        new_bitmap = (uint8_t*)kmalloc(new_bitmap_size);
        if (new_bitmap == NULL) {
            serial_puts(SERIAL_COM1, "HvFS: Failed to expand block bitmap\n");
            return -1;
        }
        if (hvfs_block_bitmap != NULL) {
            memcpy(new_bitmap, hvfs_block_bitmap, hvfs_block_bitmap_size);
            kfree(hvfs_block_bitmap);
        }
    }
    
    for (uint32_t i = hvfs_block_bitmap_size; i < new_bitmap_size; i++) {
        new_bitmap[i] = 0;
    }
    
    hvfs_block_bitmap = new_bitmap;
    hvfs_super.free_blocks += (new_count - hvfs_super.total_blocks);
    hvfs_super.total_blocks = new_count;
    hvfs_block_bitmap_size = new_bitmap_size;
    hvfs_super.dynamic_blocks = 1;
    
    serial_puts(SERIAL_COM1, "HvFS: Expanded blocks to ");
    serial_put_dec(SERIAL_COM1, new_count);
    serial_puts(SERIAL_COM1, "\n");
    
    return 0;
}

uint32_t hvfs_get_total_blocks(void) { return hvfs_super.total_blocks; }
uint32_t hvfs_get_free_blocks(void) { return hvfs_super.free_blocks; }
uint32_t hvfs_get_total_inodes(void) { return hvfs_super.inode_count; }
uint32_t hvfs_get_free_inodes(void) { return hvfs_super.free_inodes; }

void hvfs_init(void) {
    memset(&hvfs_super, 0, sizeof(struct super_block));
    
    hvfs_inode_table = NULL;
    hvfs_block_bitmap = NULL;
    hvfs_inode_bitmap = NULL;
    hvfs_inode_table_size = 0;
    hvfs_block_bitmap_size = 0;
    hvfs_inode_bitmap_size = 0;
    
    if (block_cache == NULL) {
        block_cache = (struct block_cache_entry*)kmalloc(HVFS_CACHE_SIZE * sizeof(struct block_cache_entry));
        if (block_cache) {
            for (int i = 0; i < HVFS_CACHE_SIZE; i++) {
                block_cache[i].data = (uint8_t*)kmalloc(HVFS_BLOCK_SIZE);
                if (block_cache[i].data == NULL) {
                    serial_puts(SERIAL_COM1, "HvFS: Failed to allocate cache block data\n");
                }
                block_cache[i].valid = 0;
                block_cache[i].dirty = 0;
                block_cache[i].block_num = 0;
                block_cache[i].access_time = 0;
            }
        }
    }
    
    memset(&current_context, 0, sizeof(struct hvfs_context));
    current_context.current_dir = 1;
    
    hvfs_initialized = 0;
    hvfs_disk_mode = 0;
    
    serial_puts(SERIAL_COM1, "HvFS initialized (not formatted)\n");
}

int hvfs_check_disk(void) {
    static uint8_t super_buffer[HVFS_SUPER_SECTOR_COUNT * HVFS_DISK_SECTOR_SIZE];
    struct hvfs_super_block_disk *super_disk = (struct hvfs_super_block_disk *)super_buffer;
    
    if (!ata_disk_present(0)) {
        serial_puts(SERIAL_COM1, "HvFS: No disk present\n");
        return HVFS_DISK_NO_DISK;
    }
    
    int result = ata_read_sectors(0, HVFS_SUPER_SECTOR_START, 
                         HVFS_SUPER_SECTOR_COUNT, super_buffer);
    
    if (result != 0) {
        serial_puts(SERIAL_COM1, "HvFS: Failed to read super block\n");
        return HVFS_DISK_NO_DISK;
    }
    
    if (super_disk->magic != HVFS_MAGIC) {
        serial_puts(SERIAL_COM1, "HvFS: Magic mismatch\n");
        return HVFS_DISK_UNFORMATTED;
    }
    
    if (super_disk->version > HVFS_VERSION) {
        return HVFS_DISK_VERSION_ERROR;
    }
    
    return HVFS_DISK_OK;
}

int hvfs_format_disk(void) {
    static uint8_t super_buffer[HVFS_SUPER_SECTOR_COUNT * HVFS_DISK_SECTOR_SIZE];
    struct hvfs_super_block_disk *super_disk = (struct hvfs_super_block_disk *)super_buffer;
    
    serial_puts(SERIAL_COM1, "HvFS: Formatting disk...\n");
    
    memset(super_buffer, 0, sizeof(super_buffer));
    super_disk->magic = HVFS_MAGIC;
    super_disk->version = HVFS_VERSION;
    super_disk->block_size = HVFS_BLOCK_SIZE;
    super_disk->total_blocks = HVFS_DEFAULT_BLOCKS;
    super_disk->free_blocks = HVFS_DEFAULT_BLOCKS - 100;
    super_disk->inode_count = HVFS_DEFAULT_INODES;
    super_disk->free_inodes = HVFS_DEFAULT_INODES - 1;
    super_disk->first_data_block = 0;
    super_disk->root_inode = 1;
    super_disk->block_bitmap_block = HVFS_BLOCK_BITMAP_START;
    super_disk->inode_bitmap_block = HVFS_INODE_BITMAP_START;
    super_disk->inode_table_block = HVFS_INODE_SECTOR_START;
    super_disk->created_time = get_time();
    super_disk->modified_time = get_time();
    super_disk->mount_time = get_time();
    super_disk->mount_count = 1;
    super_disk->state = 0;
    super_disk->dynamic_inodes = 1;
    super_disk->dynamic_blocks = 1;
    
    if (ata_write_sectors(0, HVFS_SUPER_SECTOR_START, 
                          HVFS_SUPER_SECTOR_COUNT, super_buffer) != 0) {
        serial_puts(SERIAL_COM1, "HvFS: Failed to write super block\n");
        return -1;
    }
    
    static uint8_t empty_sector[512];
    memset(empty_sector, 0, 512);
    
    for (int i = 0; i < HVFS_INODE_SECTOR_COUNT; i++) {
        ata_write_sector(0, HVFS_INODE_SECTOR_START + i, empty_sector);
    }
    
    for (int i = 0; i < HVFS_BLOCK_BITMAP_COUNT; i++) {
        ata_write_sector(0, HVFS_BLOCK_BITMAP_START + i, empty_sector);
    }
    
    for (int i = 0; i < HVFS_INODE_BITMAP_COUNT; i++) {
        ata_write_sector(0, HVFS_INODE_BITMAP_START + i, empty_sector);
    }
    
    serial_puts(SERIAL_COM1, "HvFS: Disk formatted successfully\n");
    return 0;
}

int hvfs_load_super(void) {
    static uint8_t super_buffer[HVFS_SUPER_SECTOR_COUNT * HVFS_DISK_SECTOR_SIZE];
    struct hvfs_super_block_disk *super_disk = (struct hvfs_super_block_disk *)super_buffer;
    
    if (ata_read_sectors(0, HVFS_SUPER_SECTOR_START, 
                         HVFS_SUPER_SECTOR_COUNT, super_buffer) != 0) {
        serial_puts(SERIAL_COM1, "HvFS: Failed to read super block\n");
        return -1;
    }
    
    if (super_disk->magic != HVFS_MAGIC) {
        return -1;
    }
    
    hvfs_super.magic = super_disk->magic;
    hvfs_super.version = super_disk->version;
    hvfs_super.block_size = super_disk->block_size;
    hvfs_super.total_blocks = super_disk->total_blocks;
    hvfs_super.free_blocks = super_disk->free_blocks;
    hvfs_super.inode_count = super_disk->inode_count;
    hvfs_super.free_inodes = super_disk->free_inodes;
    hvfs_super.first_data_block = super_disk->first_data_block;
    hvfs_super.root_inode = super_disk->root_inode;
    hvfs_super.created_time = super_disk->created_time;
    hvfs_super.modified_time = super_disk->modified_time;
    hvfs_super.mount_time = get_time();
    hvfs_super.mount_count = super_disk->mount_count + 1;
    hvfs_super.dynamic_inodes = super_disk->dynamic_inodes;
    hvfs_super.dynamic_blocks = super_disk->dynamic_blocks;
    
    serial_puts(SERIAL_COM1, "HvFS: Super block loaded\n");
    return 0;
}

int hvfs_sync_super(void) {
    static uint8_t super_buffer[HVFS_SUPER_SECTOR_COUNT * HVFS_DISK_SECTOR_SIZE];
    struct hvfs_super_block_disk *super_disk = (struct hvfs_super_block_disk *)super_buffer;
    
    memset(super_buffer, 0, sizeof(super_buffer));
    super_disk->magic = hvfs_super.magic;
    super_disk->version = hvfs_super.version;
    super_disk->block_size = hvfs_super.block_size;
    super_disk->total_blocks = hvfs_super.total_blocks;
    super_disk->free_blocks = hvfs_super.free_blocks;
    super_disk->inode_count = hvfs_super.inode_count;
    super_disk->free_inodes = hvfs_super.free_inodes;
    super_disk->first_data_block = hvfs_super.first_data_block;
    super_disk->root_inode = hvfs_super.root_inode;
    super_disk->block_bitmap_block = HVFS_BLOCK_BITMAP_START;
    super_disk->inode_bitmap_block = HVFS_INODE_BITMAP_START;
    super_disk->inode_table_block = HVFS_INODE_SECTOR_START;
    super_disk->created_time = hvfs_super.created_time;
    super_disk->modified_time = hvfs_super.modified_time;
    super_disk->mount_time = hvfs_super.mount_time;
    super_disk->mount_count = hvfs_super.mount_count;
    super_disk->state = hvfs_super.state;
    super_disk->dynamic_inodes = hvfs_super.dynamic_inodes;
    super_disk->dynamic_blocks = hvfs_super.dynamic_blocks;
    
    if (ata_write_sectors(0, HVFS_SUPER_SECTOR_START, 
                          HVFS_SUPER_SECTOR_COUNT, super_buffer) != 0) {
        serial_puts(SERIAL_COM1, "HvFS: Failed to sync super block\n");
        return -1;
    }
    
    return 0;
}

int hvfs_load_inode_table(void) {
    if (hvfs_inode_table != NULL) {
        kfree(hvfs_inode_table);
    }
    
    hvfs_inode_table = (struct inode*)kcalloc(hvfs_super.inode_count, sizeof(struct inode));
    if (hvfs_inode_table == NULL) {
        serial_puts(SERIAL_COM1, "HvFS: Failed to allocate inode table\n");
        return -1;
    }
    hvfs_inode_table_size = hvfs_super.inode_count;
    
    static uint8_t sector_buffer[512];
    
    for (uint32_t i = 0; i < hvfs_super.inode_count; i++) {
        if (ata_read_sector(0, HVFS_INODE_SECTOR_START + i, sector_buffer) != 0) {
            continue;
        }
        
        struct hvfs_inode_disk *inode_disk = (struct hvfs_inode_disk *)sector_buffer;
        
        if (inode_disk->inode_num != 0 && inode_disk->inode_num < hvfs_super.inode_count) {
            struct inode *inode = &hvfs_inode_table[inode_disk->inode_num];
            inode->inode_num = inode_disk->inode_num;
            inode->mode = inode_disk->mode;
            inode->size = inode_disk->size;
            inode->atime = inode_disk->atime;
            inode->mtime = inode_disk->mtime;
            inode->ctime = inode_disk->ctime;
            inode->owner_pwid = inode_disk->owner_pwid;
            inode->group_pwid = inode_disk->group_pwid;
            inode->pwid_perm = inode_disk->pwid_perm;
            inode->link_count = inode_disk->link_count;
            inode->ref_count = 1;
            inode->used = 1;
            inode->dirty = 0;
            inode->in_cache = 1;
            
            for (int j = 0; j < 12; j++) {
                inode->direct_blocks[j] = inode_disk->direct_blocks[j];
            }
            inode->indirect_block = inode_disk->indirect_block;
            inode->double_indirect = inode_disk->double_indirect;
            inode->triple_indirect = inode_disk->triple_indirect;
        }
    }
    
    serial_puts(SERIAL_COM1, "HvFS: Inode table loaded\n");
    return 0;
}

int hvfs_sync_inode(struct inode *inode) {
    if (inode == NULL || inode->inode_num == 0) return -1;
    
    static uint8_t sector_buffer[512];
    memset(sector_buffer, 0, sizeof(sector_buffer));
    
    struct hvfs_inode_disk *inode_disk = (struct hvfs_inode_disk *)sector_buffer;
    
    inode_disk->inode_num = inode->inode_num;
    inode_disk->mode = inode->mode;
    inode_disk->size = inode->size;
    inode_disk->blocks = (inode->size + HVFS_BLOCK_SIZE - 1) / HVFS_BLOCK_SIZE;
    inode_disk->atime = inode->atime;
    inode_disk->mtime = inode->mtime;
    inode_disk->ctime = inode->ctime;
    inode_disk->owner_pwid = inode->owner_pwid;
    inode_disk->group_pwid = inode->group_pwid;
    inode_disk->pwid_perm = inode->pwid_perm;
    inode_disk->link_count = inode->link_count;
    
    for (int i = 0; i < 12; i++) {
        inode_disk->direct_blocks[i] = inode->direct_blocks[i];
    }
    inode_disk->indirect_block = inode->indirect_block;
    inode_disk->double_indirect = inode->double_indirect;
    inode_disk->triple_indirect = inode->triple_indirect;
    inode_disk->flags = 0;
    
    if (ata_write_sector(0, HVFS_INODE_SECTOR_START + inode->inode_num, sector_buffer) != 0) {
        serial_puts(SERIAL_COM1, "HvFS: Failed to sync inode\n");
        return -1;
    }
    
    inode->dirty = 0;
    return 0;
}

int hvfs_load_block_bitmap(void) {
    uint32_t bitmap_size = (hvfs_super.total_blocks + 7) / 8;
    
    if (hvfs_block_bitmap != NULL) {
        kfree(hvfs_block_bitmap);
    }
    
    hvfs_block_bitmap = (uint8_t*)kmalloc(bitmap_size);
    if (hvfs_block_bitmap == NULL) {
        serial_puts(SERIAL_COM1, "HvFS: Failed to allocate block bitmap\n");
        return -1;
    }
    hvfs_block_bitmap_size = bitmap_size;
    
    uint32_t sectors_needed = (bitmap_size + 511) / 512;
    static uint8_t bitmap_buffer[HVFS_BLOCK_BITMAP_COUNT * HVFS_DISK_SECTOR_SIZE];
    
    if (ata_read_sectors(0, HVFS_BLOCK_BITMAP_START, 
                         sectors_needed, bitmap_buffer) != 0) {
        serial_puts(SERIAL_COM1, "HvFS: Failed to load block bitmap\n");
        return -1;
    }
    
    memcpy(hvfs_block_bitmap, bitmap_buffer, bitmap_size);
    
    serial_puts(SERIAL_COM1, "HvFS: Block bitmap loaded\n");
    return 0;
}

int hvfs_sync_block_bitmap(void) {
    uint32_t sectors_needed = (hvfs_block_bitmap_size + 511) / 512;
    static uint8_t bitmap_buffer[HVFS_BLOCK_BITMAP_COUNT * HVFS_DISK_SECTOR_SIZE];
    
    memset(bitmap_buffer, 0, sizeof(bitmap_buffer));
    memcpy(bitmap_buffer, hvfs_block_bitmap, hvfs_block_bitmap_size);
    
    if (ata_write_sectors(0, HVFS_BLOCK_BITMAP_START, 
                          sectors_needed, bitmap_buffer) != 0) {
        serial_puts(SERIAL_COM1, "HvFS: Failed to sync block bitmap\n");
        return -1;
    }
    
    return 0;
}

int hvfs_load_inode_bitmap(void) {
    uint32_t bitmap_size = (hvfs_super.inode_count + 7) / 8;
    
    if (hvfs_inode_bitmap != NULL) {
        kfree(hvfs_inode_bitmap);
    }
    
    hvfs_inode_bitmap = (uint8_t*)kmalloc(bitmap_size);
    if (hvfs_inode_bitmap == NULL) {
        serial_puts(SERIAL_COM1, "HvFS: Failed to allocate inode bitmap\n");
        return -1;
    }
    hvfs_inode_bitmap_size = bitmap_size;
    
    uint32_t sectors_needed = (bitmap_size + 511) / 512;
    static uint8_t bitmap_buffer[HVFS_INODE_BITMAP_COUNT * HVFS_DISK_SECTOR_SIZE];
    
    if (ata_read_sectors(0, HVFS_INODE_BITMAP_START, 
                         sectors_needed, bitmap_buffer) != 0) {
        serial_puts(SERIAL_COM1, "HvFS: Failed to load inode bitmap\n");
        return -1;
    }
    
    memcpy(hvfs_inode_bitmap, bitmap_buffer, bitmap_size);
    
    serial_puts(SERIAL_COM1, "HvFS: Inode bitmap loaded\n");
    return 0;
}

int hvfs_sync_inode_bitmap(void) {
    uint32_t sectors_needed = (hvfs_inode_bitmap_size + 511) / 512;
    static uint8_t bitmap_buffer[HVFS_INODE_BITMAP_COUNT * HVFS_DISK_SECTOR_SIZE];
    
    memset(bitmap_buffer, 0, sizeof(bitmap_buffer));
    memcpy(bitmap_buffer, hvfs_inode_bitmap, hvfs_inode_bitmap_size);
    
    if (ata_write_sectors(0, HVFS_INODE_BITMAP_START, 
                          sectors_needed, bitmap_buffer) != 0) {
        serial_puts(SERIAL_COM1, "HvFS: Failed to sync inode bitmap\n");
        return -1;
    }
    
    return 0;
}

static int hvfs_load_data_block(uint32_t block_num) {
    if (block_num >= hvfs_super.total_blocks) return -1;
    get_block(block_num);
    return 0;
}

static int hvfs_sync_data_block(uint32_t block_num) {
    if (block_num >= hvfs_super.total_blocks) return -1;
    
    for (int i = 0; i < HVFS_CACHE_SIZE; i++) {
        if (block_cache[i].valid && block_cache[i].block_num == block_num) {
            if (block_cache[i].dirty) {
                for (int s = 0; s < HVFS_DISK_SECTORS_PER_BLOCK; s++) {
                    ata_write_sector(0, HVFS_DATA_SECTOR_START + block_num * HVFS_DISK_SECTORS_PER_BLOCK + s,
                        block_cache[i].data + s * HVFS_DISK_SECTOR_SIZE);
                }
                block_cache[i].dirty = 0;
            }
            return 0;
        }
    }
    
    return 0;
}

int hvfs_disk_init(void) {
    int status = hvfs_check_disk();
    
    switch (status) {
        case HVFS_DISK_OK:
            serial_puts(SERIAL_COM1, "HvFS: Found valid disk filesystem\n");
            return hvfs_mount();
            
        case HVFS_DISK_NO_DISK:
            serial_puts(SERIAL_COM1, "HvFS: No disk detected, using memory mode\n");
            hvfs_disk_mode = 0;
            return -1;
            
        case HVFS_DISK_UNFORMATTED:
            serial_puts(SERIAL_COM1, "HvFS: Disk unformatted, formatting...\n");
            if (hvfs_format_disk() != 0) {
                return -1;
            }
            hvfs_format();
            hvfs_disk_mode = 1;
            hvfs_sync();
            serial_puts(SERIAL_COM1, "HvFS: Disk init complete\n");
            return 0;
            
        case HVFS_DISK_VERSION_ERROR:
            serial_puts(SERIAL_COM1, "HvFS: Disk version mismatch\n");
            return -1;
            
        default:
            return -1;
    }
}

int hvfs_mount(void) {
    if (hvfs_load_super() != 0) {
        return -1;
    }
    
    if (hvfs_load_inode_bitmap() != 0) {
        return -1;
    }
    
    if (hvfs_load_block_bitmap() != 0) {
        return -1;
    }
    
    if (hvfs_load_inode_table() != 0) {
        return -1;
    }
    
    current_context.current_dir = hvfs_super.root_inode;
    hvfs_initialized = 1;
    hvfs_disk_mode = 1;
    
    serial_puts(SERIAL_COM1, "HvFS: Mounted successfully\n");
    return 0;
}

int hvfs_unmount(void) {
    if (!hvfs_initialized) return -1;
    
    hvfs_sync();
    
    hvfs_initialized = 0;
    hvfs_disk_mode = 0;
    
    serial_puts(SERIAL_COM1, "HvFS: Unmounted\n");
    return 0;
}

int hvfs_sync(void) {
    if (!hvfs_disk_mode) return 0;
    
    serial_puts(SERIAL_COM1, "HvFS: Syncing to disk...\n");
    
    hvfs_sync_super();
    hvfs_sync_inode_bitmap();
    hvfs_sync_block_bitmap();
    
    for (uint32_t i = 1; i < hvfs_super.inode_count; i++) {
        if (hvfs_inode_table[i].used && hvfs_inode_table[i].dirty) {
            hvfs_sync_inode(&hvfs_inode_table[i]);
        }
    }
    
    for (int i = 0; i < HVFS_CACHE_SIZE; i++) {
        if (block_cache[i].valid && block_cache[i].dirty) {
            hvfs_sync_data_block(block_cache[i].block_num);
        }
    }
    
    extern int pwid_save_to_disk(void);
    extern int pwid_is_modified(void);
    if (pwid_is_modified()) {
        pwid_save_to_disk();
    }
    
    serial_puts(SERIAL_COM1, "HvFS: Sync complete\n");
    return 0;
}

int hvfs_format(void) {
    if (hvfs_inode_table != NULL) {
        kfree(hvfs_inode_table);
        hvfs_inode_table = NULL;
    }
    if (hvfs_block_bitmap != NULL) {
        kfree(hvfs_block_bitmap);
        hvfs_block_bitmap = NULL;
    }
    if (hvfs_inode_bitmap != NULL) {
        kfree(hvfs_inode_bitmap);
        hvfs_inode_bitmap = NULL;
    }
    
    if (block_cache == NULL) {
        block_cache = (struct block_cache_entry*)kmalloc(HVFS_CACHE_SIZE * sizeof(struct block_cache_entry));
        if (block_cache == NULL) {
            serial_puts(SERIAL_COM1, "HvFS: Failed to allocate block cache\n");
            return -1;
        }
        for (int i = 0; i < HVFS_CACHE_SIZE; i++) {
            block_cache[i].data = (uint8_t*)kmalloc(HVFS_BLOCK_SIZE);
            if (block_cache[i].data == NULL) {
                serial_puts(SERIAL_COM1, "HvFS: Failed to allocate cache data block\n");
                return -1;
            }
            block_cache[i].valid = 0;
            block_cache[i].dirty = 0;
            block_cache[i].block_num = 0;
            block_cache[i].access_time = 0;
        }
    }
    
    memset(&hvfs_super, 0, sizeof(struct super_block));
    
    hvfs_super.magic = HVFS_MAGIC;
    hvfs_super.version = HVFS_VERSION;
    hvfs_super.block_size = HVFS_BLOCK_SIZE;
    hvfs_super.total_blocks = HVFS_DEFAULT_BLOCKS;
    hvfs_super.free_blocks = HVFS_DEFAULT_BLOCKS - 100;
    hvfs_super.inode_count = HVFS_DEFAULT_INODES;
    hvfs_super.free_inodes = HVFS_DEFAULT_INODES - 2;
    hvfs_super.first_data_block = 10;
    hvfs_super.root_inode = 1;
    hvfs_super.max_path_depth = 256;
    hvfs_super.max_entries = 1048576;
    hvfs_super.created_time = get_time();
    hvfs_super.modified_time = get_time();
    hvfs_super.dynamic_inodes = 1;
    hvfs_super.dynamic_blocks = 1;
    
    hvfs_inode_table = (struct inode*)kcalloc(hvfs_super.inode_count, sizeof(struct inode));
    if (hvfs_inode_table == NULL) {
        serial_puts(SERIAL_COM1, "[HvFS] format: failed to allocate inode table\n");
        return -1;
    }
    
    hvfs_block_bitmap = (uint8_t*)kcalloc((hvfs_super.total_blocks + 7) / 8, 1);
    if (hvfs_block_bitmap == NULL) {
        serial_puts(SERIAL_COM1, "[HvFS] format: failed to allocate block bitmap\n");
        return -1;
    }
    
    hvfs_inode_bitmap = (uint8_t*)kcalloc((hvfs_super.inode_count + 7) / 8, 1);
    if (hvfs_inode_bitmap == NULL) {
        serial_puts(SERIAL_COM1, "[HvFS] format: failed to allocate inode bitmap\n");
        return -1;
    }
    
    hvfs_inode_table_size = hvfs_super.inode_count;
    hvfs_block_bitmap_size = (hvfs_super.total_blocks + 7) / 8;
    hvfs_inode_bitmap_size = (hvfs_super.inode_count + 7) / 8;
    
    for (int i = 0; i < hvfs_super.first_data_block; i++) {
        block_set_used(i);
    }
    
    struct inode *root = &hvfs_inode_table[1];
    root->inode_num = 1;
    root->mode = HVFS_TYPE_DIR | 0755;
    root->size = 0;
    root->atime = get_time();
    root->mtime = get_time();
    root->ctime = get_time();
    root->owner_pwid = 0;
    root->group_pwid = 0;
    root->pwid_perm = (0x07 << 8) | 0x05;
    
    root->direct_blocks[0] = block_alloc();
    if (root->direct_blocks[0] == 0) {
        serial_puts(SERIAL_COM1, "[HvFS] format: failed to allocate block for root\n");
        return -1;
    }
    root->link_count = 2;
    root->ref_count = 1;
    root->used = 1;
    root->dirty = 0;
    root->in_cache = 1;
    inode_set_used(1);
    
    struct inode *lost_found = &hvfs_inode_table[2];
    lost_found->inode_num = 2;
    lost_found->mode = HVFS_TYPE_DIR | 0755;
    lost_found->size = 0;
    lost_found->atime = get_time();
    lost_found->mtime = get_time();
    lost_found->ctime = get_time();
    lost_found->owner_pwid = 0;
    lost_found->group_pwid = 0;
    lost_found->pwid_perm = (0x07 << 8) | 0x05;
    lost_found->direct_blocks[0] = block_alloc();
    if (lost_found->direct_blocks[0] == 0) {
        serial_puts(SERIAL_COM1, "[HvFS] format: failed to allocate block for lost+found\n");
        return -1;
    }
    lost_found->link_count = 2;
    lost_found->ref_count = 1;
    lost_found->used = 1;
    lost_found->dirty = 0;
    lost_found->in_cache = 1;
    inode_set_used(2);
    
    struct dir_entry *root_dir = (struct dir_entry *)get_block(root->direct_blocks[0]);
    if (root_dir == NULL) {
        serial_puts(SERIAL_COM1, "[HvFS] format: failed to get root directory block\n");
        return -1;
    }
    
    root_dir[0].inode = 1;
    root_dir[0].rec_len = sizeof(struct dir_entry);
    root_dir[0].name_len = 1;
    root_dir[0].file_type = HVFS_TYPE_DIR;
    strcpy(root_dir[0].name, ".");
    
    root_dir[1].inode = 1;
    root_dir[1].rec_len = sizeof(struct dir_entry);
    root_dir[1].name_len = 2;
    root_dir[1].file_type = HVFS_TYPE_DIR;
    strcpy(root_dir[1].name, "..");
    
    root_dir[2].inode = 2;
    root_dir[2].rec_len = sizeof(struct dir_entry);
    root_dir[2].name_len = 10;
    root_dir[2].file_type = HVFS_TYPE_DIR;
    strcpy(root_dir[2].name, "lost+found");
    
    root->size = 3 * sizeof(struct dir_entry);
    mark_block_dirty(root->direct_blocks[0]);
    
    current_context.current_dir = 1;
    hvfs_initialized = 1;
    
    hvfs_create_default_directories();
    
    serial_puts(SERIAL_COM1, "HvFS formatted successfully\n");
    serial_puts(SERIAL_COM1, "  Block size: ");
    serial_put_dec(SERIAL_COM1, hvfs_super.block_size);
    serial_puts(SERIAL_COM1, " bytes\n  Total blocks: ");
    serial_put_dec(SERIAL_COM1, hvfs_super.total_blocks);
    serial_puts(SERIAL_COM1, "\n  Free blocks: ");
    serial_put_dec(SERIAL_COM1, hvfs_super.free_blocks);
    serial_puts(SERIAL_COM1, "\n  Total inodes: ");
    serial_put_dec(SERIAL_COM1, hvfs_super.inode_count);
    serial_puts(SERIAL_COM1, "\n  Root inode: ");
    serial_put_dec(SERIAL_COM1, hvfs_super.root_inode);
    serial_puts(SERIAL_COM1, "\n");
    
    return 0;
}

int hvfs_create_default_directories(void) {
    uint64_t root_pwid = 0;
    extern struct pwid_entry *pwid_find_by_note(const char *note);
    struct pwid_entry *root = pwid_find_by_note("root");
    if (root) {
        root_pwid = root->pwid;
    }
    
    const char *dirs[] = {
        "/bin",
        "/sbin",
        "/etc",
        "/home",
        "/tmp",
        "/dev",
        "/proc",
        "/sys",
        "/var",
        "/usr",
        "/opt",
        NULL
    };
    
    for (int i = 0; dirs[i] != NULL; i++) {
        hvfs_mkdir(dirs[i], root_pwid);
    }
    
    int fd = hvfs_open("/etc/pwid.db", HVFS_O_CREAT | HVFS_O_WRONLY, root_pwid);
    if (fd >= 0) {
        hvfs_close(fd);
    }
    
    fd = hvfs_open("/etc/hostname", HVFS_O_CREAT | HVFS_O_WRONLY, root_pwid);
    if (fd >= 0) {
        const char *default_hostname = "localhost";
        hvfs_write(fd, default_hostname, strlen(default_hostname));
        hvfs_close(fd);
    }
    
    return 0;
}

static struct inode* resolve_path(const char *path, uint64_t pwid) {
    if (!hvfs_initialized) return NULL;
    
    struct inode *current;
    const char *p = path;
    
    if (*p == '/') {
        current = hvfs_get_inode(hvfs_super.root_inode);
        p++;
    } else {
        current = hvfs_get_inode(current_context.current_dir);
    }
    
    if (current == NULL) return NULL;
    
    while (*p) {
        while (*p == '/') p++;
        if (*p == '\0') break;
        
        if ((current->mode & 0xF000) != (HVFS_TYPE_DIR << 12)) {
            return NULL;
        }
        
        char name[HVFS_MAX_NAME];
        int name_len = 0;
        while (*p && *p != '/' && name_len < HVFS_MAX_NAME - 1) {
            name[name_len++] = *p++;
        }
        name[name_len] = '\0';
        
        int found = 0;
        
        struct dir_entry *entries = (struct dir_entry *)get_block(current->direct_blocks[0]);
        if (entries == NULL) return NULL;
        
        uint32_t num_entries = current->size / sizeof(struct dir_entry);
        
        for (uint32_t i = 0; i < num_entries; i++) {
            if (entries[i].inode != 0 && strcmp(entries[i].name, name) == 0) {
                current = hvfs_get_inode(entries[i].inode);
                found = 1;
                break;
            }
        }
        
        if (!found) return NULL;
    }
    
    return current;
}

int hvfs_open(const char *path, int flags, uint64_t pwid) {
    if (!hvfs_initialized) return -1;
    
    struct inode *inode = resolve_path(path, pwid);
    
    if (inode == NULL) {
        if (flags & HVFS_O_CREAT) {
            const char *filename = path;
            const char *last_slash = path;
            for (const char *p = path; *p; p++) {
                if (*p == '/') last_slash = p + 1;
            }
            filename = last_slash;
            
            char dir_path[HVFS_MAX_PATH];
            int dir_len = last_slash - path;
            if (dir_len == 0) {
                dir_path[0] = '/';
                dir_path[1] = '\0';
            } else {
                memcpy(dir_path, path, dir_len);
                dir_path[dir_len] = '\0';
            }
            
            struct inode *parent = resolve_path(dir_path, pwid);
            if (parent == NULL) return -1;
            
            if (!hvfs_check_permission(parent, pwid, HVFS_PERM_W)) {
                return -1;
            }
            
            inode = inode_alloc();
            if (inode == NULL) return -1;
            
            inode->mode = HVFS_TYPE_FILE | 0644;
            inode->size = 0;
            inode->atime = get_time();
            inode->mtime = get_time();
            inode->ctime = get_time();
            inode->owner_pwid = pwid;
            inode->group_pwid = 0;
            inode->pwid_perm = (0x06 << 8) | 0x04;
            inode->direct_blocks[0] = block_alloc();
            inode->dirty = 1;
            
            struct dir_entry *entries = (struct dir_entry *)get_block(parent->direct_blocks[0]);
            if (entries == NULL) {
                inode_free(inode);
                return -1;
            }
            
            uint32_t num_entries = parent->size / sizeof(struct dir_entry);
            
            entries[num_entries].inode = inode->inode_num;
            entries[num_entries].rec_len = sizeof(struct dir_entry);
            entries[num_entries].name_len = strlen(filename);
            entries[num_entries].file_type = HVFS_TYPE_FILE;
            strcpy(entries[num_entries].name, filename);
            
            parent->size += sizeof(struct dir_entry);
            parent->mtime = get_time();
            parent->dirty = 1;
            mark_block_dirty(parent->direct_blocks[0]);
            
            if (hvfs_disk_mode) {
                hvfs_sync_inode(parent);
                hvfs_sync_inode(inode);
            }
        } else {
            return -1;
        }
    }
    
    if (!hvfs_check_permission(inode, pwid, HVFS_PERM_R)) {
        return -1;
    }
    
    int fd = -1;
    for (int i = 0; i < HVFS_MAX_FDS; i++) {
        if (!current_context.fds[i].used) {
            fd = i;
            break;
        }
    }
    
    if (fd < 0) return -1;
    
    current_context.fds[fd].fd = next_fd++;
    current_context.fds[fd].inode_num = inode->inode_num;
    current_context.fds[fd].offset = (flags & HVFS_O_APPEND) ? inode->size : 0;
    current_context.fds[fd].flags = flags;
    current_context.fds[fd].pwid = pwid;
    current_context.fds[fd].used = 1;
    
    inode->ref_count++;
    
    return current_context.fds[fd].fd;
}

int hvfs_close(int fd) {
    for (int i = 0; i < HVFS_MAX_FDS; i++) {
        if (current_context.fds[i].used && current_context.fds[i].fd == fd) {
            struct inode *inode = hvfs_get_inode(current_context.fds[i].inode_num);
            if (inode != NULL) {
                inode->ref_count--;
                
                if (hvfs_disk_mode && inode->dirty) {
                    hvfs_sync_inode(inode);
                }
            }
            current_context.fds[i].used = 0;
            return 0;
        }
    }
    return -1;
}

int hvfs_read(int fd, void *buf, uint32_t count) {
    struct file_descriptor *fdesc = NULL;
    for (int i = 0; i < HVFS_MAX_FDS; i++) {
        if (current_context.fds[i].used && current_context.fds[i].fd == fd) {
            fdesc = &current_context.fds[i];
            break;
        }
    }
    
    if (fdesc == NULL) return -1;
    
    struct inode *inode = hvfs_get_inode(fdesc->inode_num);
    if (inode == NULL) return -1;
    
    if (!hvfs_check_permission(inode, fdesc->pwid, HVFS_PERM_R)) {
        return -1;
    }
    
    uint32_t bytes_read = 0;
    uint8_t *buffer = (uint8_t *)buf;
    
    while (bytes_read < count && fdesc->offset < inode->size) {
        uint32_t block_idx = fdesc->offset / HVFS_BLOCK_SIZE;
        uint32_t block_offset = fdesc->offset % HVFS_BLOCK_SIZE;
        uint32_t bytes_to_read = HVFS_BLOCK_SIZE - block_offset;
        
        if (bytes_to_read > count - bytes_read) {
            bytes_to_read = count - bytes_read;
        }
        if (bytes_to_read > inode->size - fdesc->offset) {
            bytes_to_read = inode->size - fdesc->offset;
        }
        
        uint32_t block_num = get_block_for_index(inode, block_idx, 0);
        if (block_num != 0) {
            uint8_t *block = get_block(block_num);
            if (block) {
                memcpy(buffer + bytes_read, block + block_offset, bytes_to_read);
            }
        }
        
        bytes_read += bytes_to_read;
        fdesc->offset += bytes_to_read;
    }
    
    inode->atime = get_time();
    return bytes_read;
}

int hvfs_write(int fd, const void *buf, uint32_t count) {
    struct file_descriptor *fdesc = NULL;
    for (int i = 0; i < HVFS_MAX_FDS; i++) {
        if (current_context.fds[i].used && current_context.fds[i].fd == fd) {
            fdesc = &current_context.fds[i];
            break;
        }
    }
    
    if (fdesc == NULL) return -1;
    
    struct inode *inode = hvfs_get_inode(fdesc->inode_num);
    if (inode == NULL) return -1;
    
    if (!hvfs_check_permission(inode, fdesc->pwid, HVFS_PERM_W)) {
        return -1;
    }
    
    uint32_t bytes_written = 0;
    const uint8_t *buffer = (const uint8_t *)buf;
    uint64_t max_blocks = 12 + INDIRECT_BLOCKS_PER_BLOCK + 
                          INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK +
                          INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK;
    
    while (bytes_written < count) {
        uint32_t block_idx = fdesc->offset / HVFS_BLOCK_SIZE;
        uint32_t block_offset = fdesc->offset % HVFS_BLOCK_SIZE;
        uint32_t bytes_to_write = HVFS_BLOCK_SIZE - block_offset;
        
        if (bytes_to_write > count - bytes_written) {
            bytes_to_write = count - bytes_written;
        }
        
        if (block_idx >= max_blocks) break;
        
        uint32_t block_num = get_block_for_index(inode, block_idx, 1);
        if (block_num == 0) break;
        
        uint8_t *block = get_block(block_num);
        if (block == NULL) break;
        
        memcpy(block + block_offset, buffer + bytes_written, bytes_to_write);
        mark_block_dirty(block_num);
        
        bytes_written += bytes_to_write;
        fdesc->offset += bytes_to_write;
        
        if (fdesc->offset > inode->size) {
            inode->size = fdesc->offset;
        }
    }
    
    inode->mtime = get_time();
    inode->dirty = 1;
    hvfs_super.modified_time = get_time();
    
    if (hvfs_disk_mode) {
        hvfs_sync_inode(inode);
    }
    
    return bytes_written;
}

int hvfs_seek(int fd, int64_t offset, int whence) {
    struct file_descriptor *fdesc = NULL;
    for (int i = 0; i < HVFS_MAX_FDS; i++) {
        if (current_context.fds[i].used && current_context.fds[i].fd == fd) {
            fdesc = &current_context.fds[i];
            break;
        }
    }
    
    if (fdesc == NULL) return -1;
    
    struct inode *inode = hvfs_get_inode(fdesc->inode_num);
    if (inode == NULL) return -1;
    
    int64_t new_offset;
    
    switch (whence) {
        case 0:
            new_offset = offset;
            break;
        case 1:
            new_offset = fdesc->offset + offset;
            break;
        case 2:
            new_offset = inode->size + offset;
            break;
        default:
            return -1;
    }
    
    if (new_offset < 0) new_offset = 0;
    if (new_offset > inode->size) new_offset = inode->size;
    
    fdesc->offset = new_offset;
    return new_offset;
}

int hvfs_mkdir(const char *path, uint64_t pwid) {
    if (!hvfs_initialized) return -1;
    
    const char *dirname = path;
    const char *last_slash = path;
    for (const char *p = path; *p; p++) {
        if (*p == '/') last_slash = p + 1;
    }
    dirname = last_slash;
    
    char parent_path[HVFS_MAX_PATH];
    int parent_len = last_slash - path;
    if (parent_len == 0) {
        parent_path[0] = '/';
        parent_path[1] = '\0';
    } else {
        memcpy(parent_path, path, parent_len);
        parent_path[parent_len] = '\0';
    }
    
    struct inode *parent = resolve_path(parent_path, pwid);
    if (parent == NULL) return -1;
    
    if (!hvfs_check_permission(parent, pwid, HVFS_PERM_W)) {
        return -1;
    }
    
    struct inode *new_dir = inode_alloc();
    if (new_dir == NULL) return -1;
    
    new_dir->mode = HVFS_TYPE_DIR | 0755;
    new_dir->size = 2 * sizeof(struct dir_entry);
    new_dir->atime = get_time();
    new_dir->mtime = get_time();
    new_dir->ctime = get_time();
    new_dir->owner_pwid = pwid;
    new_dir->group_pwid = 0;
    new_dir->pwid_perm = (0x07 << 8) | 0x05;
    new_dir->direct_blocks[0] = block_alloc();
    new_dir->link_count = 2;
    new_dir->dirty = 1;
    
    struct dir_entry *new_entries = (struct dir_entry *)get_block(new_dir->direct_blocks[0]);
    if (new_entries) {
        new_entries[0].inode = new_dir->inode_num;
        new_entries[0].rec_len = sizeof(struct dir_entry);
        new_entries[0].name_len = 1;
        new_entries[0].file_type = HVFS_TYPE_DIR;
        strcpy(new_entries[0].name, ".");
        
        new_entries[1].inode = parent->inode_num;
        new_entries[1].rec_len = sizeof(struct dir_entry);
        new_entries[1].name_len = 2;
        new_entries[1].file_type = HVFS_TYPE_DIR;
        strcpy(new_entries[1].name, "..");
        
        mark_block_dirty(new_dir->direct_blocks[0]);
    }
    
    struct dir_entry *parent_entries = (struct dir_entry *)get_block(parent->direct_blocks[0]);
    if (parent_entries == NULL) {
        inode_free(new_dir);
        return -1;
    }
    
    uint32_t num_entries = parent->size / sizeof(struct dir_entry);
    
    parent_entries[num_entries].inode = new_dir->inode_num;
    parent_entries[num_entries].rec_len = sizeof(struct dir_entry);
    parent_entries[num_entries].name_len = strlen(dirname);
    parent_entries[num_entries].file_type = HVFS_TYPE_DIR;
    strcpy(parent_entries[num_entries].name, dirname);
    
    parent->size += sizeof(struct dir_entry);
    parent->link_count++;
    parent->mtime = get_time();
    parent->dirty = 1;
    mark_block_dirty(parent->direct_blocks[0]);
    
    if (hvfs_disk_mode) {
        hvfs_sync_inode(new_dir);
        hvfs_sync_inode(parent);
    }
    
    return 0;
}

int hvfs_rmdir(const char *path, uint64_t pwid) {
    if (!hvfs_initialized) return -1;
    
    struct inode *dir = resolve_path(path, pwid);
    if (dir == NULL) return -1;
    
    if ((dir->mode & 0xF000) != (HVFS_TYPE_DIR << 12)) {
        return -1;
    }
    
    if (dir->inode_num == hvfs_super.root_inode) {
        return -1;
    }
    
    if (dir->size > 2 * sizeof(struct dir_entry)) {
        return -1;
    }
    
    if (!hvfs_check_permission(dir, pwid, HVFS_PERM_W)) {
        return -1;
    }
    
    const char *dirname = path;
    const char *last_slash = path;
    for (const char *p = path; *p; p++) {
        if (*p == '/') last_slash = p + 1;
    }
    dirname = last_slash;
    
    char parent_path[HVFS_MAX_PATH];
    int parent_len = last_slash - path;
    if (parent_len == 0) {
        parent_path[0] = '/';
        parent_path[1] = '\0';
    } else {
        memcpy(parent_path, path, parent_len);
        parent_path[parent_len] = '\0';
    }
    
    struct inode *parent = resolve_path(parent_path, pwid);
    if (parent == NULL) return -1;
    
    struct dir_entry *entries = (struct dir_entry *)get_block(parent->direct_blocks[0]);
    if (entries == NULL) return -1;
    
    uint32_t num_entries = parent->size / sizeof(struct dir_entry);
    
    for (uint32_t i = 0; i < num_entries; i++) {
        if (strcmp(entries[i].name, dirname) == 0) {
            entries[i].inode = 0;
            break;
        }
    }
    
    parent->link_count--;
    parent->dirty = 1;
    mark_block_dirty(parent->direct_blocks[0]);
    
    if (hvfs_disk_mode) {
        hvfs_sync_inode(parent);
    }
    
    inode_free(dir);
    
    return 0;
}

int hvfs_readdir(int fd, struct dir_entry *entry) {
    struct file_descriptor *fdesc = NULL;
    for (int i = 0; i < HVFS_MAX_FDS; i++) {
        if (current_context.fds[i].used && current_context.fds[i].fd == fd) {
            fdesc = &current_context.fds[i];
            break;
        }
    }
    
    if (fdesc == NULL) return -1;
    
    struct inode *inode = hvfs_get_inode(fdesc->inode_num);
    if (inode == NULL) return -1;
    
    if ((inode->mode & 0xF000) != (HVFS_TYPE_DIR << 12)) {
        return -1;
    }
    
    struct dir_entry *entries = (struct dir_entry *)get_block(inode->direct_blocks[0]);
    if (entries == NULL) return -1;
    
    uint32_t num_entries = inode->size / sizeof(struct dir_entry);
    
    uint32_t entry_idx = fdesc->offset / sizeof(struct dir_entry);
    
    while (entry_idx < num_entries && entries[entry_idx].inode == 0) {
        entry_idx++;
        fdesc->offset += sizeof(struct dir_entry);
    }
    
    if (entry_idx >= num_entries) {
        return 0;
    }
    
    memcpy(entry, &entries[entry_idx], sizeof(struct dir_entry));
    fdesc->offset += sizeof(struct dir_entry);
    
    return 1;
}

int hvfs_stat(const char *path, struct inode *stat_buf, uint64_t pwid) {
    if (!hvfs_initialized) return -1;
    
    struct inode *inode = resolve_path(path, pwid);
    if (inode == NULL) return -1;
    
    if (!hvfs_check_permission(inode, pwid, HVFS_PERM_R)) {
        return -1;
    }
    
    memcpy(stat_buf, inode, sizeof(struct inode));
    return 0;
}

int hvfs_unlink(const char *path, uint64_t pwid) {
    if (!hvfs_initialized) return -1;
    
    struct inode *file = resolve_path(path, pwid);
    if (file == NULL) return -1;
    
    if ((file->mode & 0xF000) == (HVFS_TYPE_DIR << 12)) {
        return -1;
    }
    
    if (!hvfs_check_permission(file, pwid, HVFS_PERM_W)) {
        return -1;
    }
    
    const char *filename = path;
    const char *last_slash = path;
    for (const char *p = path; *p; p++) {
        if (*p == '/') last_slash = p + 1;
    }
    filename = last_slash;
    
    char parent_path[HVFS_MAX_PATH];
    int parent_len = last_slash - path;
    if (parent_len == 0) {
        parent_path[0] = '/';
        parent_path[1] = '\0';
    } else {
        memcpy(parent_path, path, parent_len);
        parent_path[parent_len] = '\0';
    }
    
    struct inode *parent = resolve_path(parent_path, pwid);
    if (parent == NULL) return -1;
    
    struct dir_entry *entries = (struct dir_entry *)get_block(parent->direct_blocks[0]);
    if (entries == NULL) return -1;
    
    uint32_t num_entries = parent->size / sizeof(struct dir_entry);
    
    for (uint32_t i = 0; i < num_entries; i++) {
        if (strcmp(entries[i].name, filename) == 0) {
            entries[i].inode = 0;
            break;
        }
    }
    
    parent->dirty = 1;
    mark_block_dirty(parent->direct_blocks[0]);
    
    if (hvfs_disk_mode) {
        hvfs_sync_inode(parent);
    }
    
    file->link_count--;
    file->dirty = 1;
    inode_free(file);
    
    return 0;
}

int hvfs_rename(const char *old_path, const char *new_path, uint64_t pwid) {
    if (!hvfs_initialized) return -1;
    
    const char *old_name = old_path;
    const char *old_last_slash = old_path;
    for (const char *p = old_path; *p; p++) {
        if (*p == '/') old_last_slash = p + 1;
    }
    old_name = old_last_slash;
    
    char old_parent_path[HVFS_MAX_PATH];
    int old_parent_len = old_last_slash - old_path;
    if (old_parent_len == 0) {
        old_parent_path[0] = '/';
        old_parent_path[1] = '\0';
    } else {
        memcpy(old_parent_path, old_path, old_parent_len);
        old_parent_path[old_parent_len] = '\0';
    }
    
    struct inode *old_parent = resolve_path(old_parent_path, pwid);
    if (old_parent == NULL) return -1;
    
    if (!hvfs_check_permission(old_parent, pwid, HVFS_PERM_W)) {
        return -1;
    }
    
    struct dir_entry *old_entries = (struct dir_entry *)get_block(old_parent->direct_blocks[0]);
    if (old_entries == NULL) return -1;
    
    uint32_t old_num_entries = old_parent->size / sizeof(struct dir_entry);
    
    struct inode *target_inode = NULL;
    for (uint32_t i = 0; i < old_num_entries; i++) {
        if (strcmp(old_entries[i].name, old_name) == 0 && old_entries[i].inode != 0) {
            target_inode = hvfs_get_inode(old_entries[i].inode);
            old_entries[i].inode = 0;
            break;
        }
    }
    
    if (target_inode == NULL) return -1;
    
    const char *new_name = new_path;
    const char *new_last_slash = new_path;
    for (const char *p = new_path; *p; p++) {
        if (*p == '/') new_last_slash = p + 1;
    }
    new_name = new_last_slash;
    
    char new_parent_path[HVFS_MAX_PATH];
    int new_parent_len = new_last_slash - new_path;
    if (new_parent_len == 0) {
        new_parent_path[0] = '/';
        new_parent_path[1] = '\0';
    } else {
        memcpy(new_parent_path, new_path, new_parent_len);
        new_parent_path[new_parent_len] = '\0';
    }
    
    struct inode *new_parent = resolve_path(new_parent_path, pwid);
    if (new_parent == NULL) return -1;
    
    if (!hvfs_check_permission(new_parent, pwid, HVFS_PERM_W)) {
        return -1;
    }
    
    struct dir_entry *new_entries = (struct dir_entry *)get_block(new_parent->direct_blocks[0]);
    if (new_entries == NULL) return -1;
    
    uint32_t new_num_entries = new_parent->size / sizeof(struct dir_entry);
    
    int insert_pos = -1;
    for (uint32_t i = 0; i < new_num_entries; i++) {
        if (new_entries[i].inode == 0) {
            insert_pos = i;
            break;
        }
    }
    
    if (insert_pos == -1) {
        insert_pos = new_num_entries;
        new_parent->size += sizeof(struct dir_entry);
    }
    
    new_entries[insert_pos].inode = target_inode->inode_num;
    new_entries[insert_pos].rec_len = sizeof(struct dir_entry);
    new_entries[insert_pos].name_len = strlen(new_name);
    new_entries[insert_pos].file_type = (target_inode->mode >> 12) & 0xF;
    strcpy(new_entries[insert_pos].name, new_name);
    
    new_parent->mtime = get_time();
    old_parent->mtime = get_time();
    new_parent->dirty = 1;
    old_parent->dirty = 1;
    mark_block_dirty(old_parent->direct_blocks[0]);
    mark_block_dirty(new_parent->direct_blocks[0]);
    
    if (hvfs_disk_mode) {
        hvfs_sync_inode(old_parent);
        hvfs_sync_inode(new_parent);
    }
    
    return 0;
}

int hvfs_chmod(const char *path, uint16_t mode, uint64_t pwid) {
    if (!hvfs_initialized) return -1;
    
    struct inode *inode = resolve_path(path, pwid);
    if (inode == NULL) return -1;
    
    uint8_t level = pwid_get_level(pwid);
    if (level != PWID_LEVEL_ROOT && inode->owner_pwid != pwid) {
        return -1;
    }
    
    inode->pwid_perm = mode;
    inode->mtime = get_time();
    inode->dirty = 1;
    
    if (hvfs_disk_mode) {
        hvfs_sync_inode(inode);
    }
    
    return 0;
}

int hvfs_chown(const char *path, uint64_t owner_pwid, uint64_t pwid) {
    if (!hvfs_initialized) return -1;
    
    struct inode *inode = resolve_path(path, pwid);
    if (inode == NULL) return -1;
    
    uint8_t level = pwid_get_level(pwid);
    if (level != PWID_LEVEL_ROOT) {
        return -1;
    }
    
    inode->owner_pwid = owner_pwid;
    inode->mtime = get_time();
    inode->dirty = 1;
    
    if (hvfs_disk_mode) {
        hvfs_sync_inode(inode);
    }
    
    return 0;
}

void hvfs_set_context(uint64_t pwid) {
    current_context.current_pwid = pwid;
}

uint64_t hvfs_get_current_pwid(void) {
    return current_context.current_pwid;
}

uint32_t hvfs_get_current_dir(void) {
    return current_context.current_dir;
}

void hvfs_set_current_dir(uint32_t inode_num) {
    if (hvfs_get_inode(inode_num) != NULL) {
        current_context.current_dir = inode_num;
    }
}

void hvfs_list_root(void) {
    if (!hvfs_initialized) {
        serial_puts(SERIAL_COM1, "HvFS: not formatted\n");
        return;
    }
    
    struct inode *root = hvfs_get_inode(hvfs_super.root_inode);
    if (root == NULL) {
        serial_puts(SERIAL_COM1, "HvFS: root not found\n");
        return;
    }
    
    serial_puts(SERIAL_COM1, "\n=== HvFS Root Directory ===\n");
    
    struct dir_entry *entries = (struct dir_entry *)get_block(root->direct_blocks[0]);
    if (entries == NULL) {
        serial_puts(SERIAL_COM1, "HvFS: cannot read root directory\n");
        return;
    }
    
    uint32_t num_entries = root->size / sizeof(struct dir_entry);
    
    for (uint32_t i = 0; i < num_entries; i++) {
        if (entries[i].inode != 0) {
            serial_puts(SERIAL_COM1, "  ");
            if (entries[i].file_type == HVFS_TYPE_DIR) {
                serial_puts(SERIAL_COM1, "[DIR]  ");
            } else {
                serial_puts(SERIAL_COM1, "[FILE] ");
            }
            serial_puts(SERIAL_COM1, entries[i].name);
            serial_puts(SERIAL_COM1, "\n");
        }
    }
    
    serial_puts(SERIAL_COM1, "===========================\n");
}

void hvfs_dump_super(void) {
    serial_puts(SERIAL_COM1, "\n=== HvFS Super Block ===\n");
    serial_puts(SERIAL_COM1, "  Magic: 0x");
    serial_put_hex(SERIAL_COM1, hvfs_super.magic);
    serial_puts(SERIAL_COM1, "\n  Version: ");
    serial_put_dec(SERIAL_COM1, hvfs_super.version);
    serial_puts(SERIAL_COM1, "\n  Block size: ");
    serial_put_dec(SERIAL_COM1, hvfs_super.block_size);
    serial_puts(SERIAL_COM1, " bytes\n  Total blocks: ");
    serial_put_dec(SERIAL_COM1, hvfs_super.total_blocks);
    serial_puts(SERIAL_COM1, "\n  Free blocks: ");
    serial_put_dec(SERIAL_COM1, hvfs_super.free_blocks);
    serial_puts(SERIAL_COM1, "\n  Total inodes: ");
    serial_put_dec(SERIAL_COM1, hvfs_super.inode_count);
    serial_puts(SERIAL_COM1, "\n  Free inodes: ");
    serial_put_dec(SERIAL_COM1, hvfs_super.free_inodes);
    serial_puts(SERIAL_COM1, "\n  Disk mode: ");
    serial_puts(SERIAL_COM1, hvfs_disk_mode ? "yes" : "no");
    serial_puts(SERIAL_COM1, "\n  Dynamic inodes: ");
    serial_puts(SERIAL_COM1, hvfs_super.dynamic_inodes ? "yes" : "no");
    serial_puts(SERIAL_COM1, "\n  Dynamic blocks: ");
    serial_puts(SERIAL_COM1, hvfs_super.dynamic_blocks ? "yes" : "no");
    serial_puts(SERIAL_COM1, "\n========================\n");
}
