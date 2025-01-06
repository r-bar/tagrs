tag new_tag:
  if [ "$(git rev-parse HEAD)" != "$(git rev-parse master)" ]; then \
    echo "Can only release from the master branch"; exit 1; \
  fi
  git reset
  sed -i 's/^version = .*/version = "{{ new_tag }}"/' Cargo.toml
  git add Cargo.toml
  git commit -m 'tag version {{ new_tag }}' || echo 'No changes detected, continuing'
  git push
  git tag {{ new_tag }}
  git push origin {{ new_tag }}
